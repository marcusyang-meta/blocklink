//! Shared filesystem engine. One Workspace owns the root lease for its lifetime.
//! There are no shell commands, Node processes, or desktop dependencies here.
use blocklink_model::{filename_key, validate_hash, validate_uuid, Instance, LinkMode, Lockfile};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub use blocklink_model;
pub type Result<T> = std::result::Result<T, Error>;
const MAX_JSON: u64 = 16 * 1024 * 1024;
const MAX_BLOB: u64 = 1024 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Workspace is already in use")]
    Busy,
    #[error("Unsafe filesystem entry: {0}")]
    UnsafePath(String),
    #[error("Hash or size mismatch: {0}")]
    HashMismatch(String),
    #[error("Unmanaged filename collision: {0}")]
    Collision(String),
    #[error("Recovery required: {0}")]
    RecoveryRequired(String),
    #[error("Invalid input: {0}")]
    Invalid(String),
    #[error(transparent)]
    Validation(#[from] blocklink_model::ValidationError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Busy => "WORKSPACE_BUSY",
            Self::UnsafePath(_) => "UNSAFE_PATH",
            Self::HashMismatch(_) => "HASH_MISMATCH",
            Self::Collision(_) => "MOD_CONFLICT",
            Self::RecoveryRequired(_) => "RECOVERY_REQUIRED",
            Self::Invalid(_) | Self::Validation(_) | Self::Json(_) => "INVALID_INPUT",
            Self::Io(_) => "IO_ERROR",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Blob {
    pub sha512: String,
    pub bytes: u64,
    pub reused: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub schema_version: u32,
    pub generation: String,
    pub lock_sha512: String,
    pub linked: usize,
    #[serde(default)]
    pub cloned: usize,
    pub copied: usize,
    pub preserved: usize,
    pub fallback_reasons: Vec<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checkpoint {
    Prepared,
    OldMoved,
    NewMoved,
    LockWritten,
    ReceiptWritten,
    Committed,
}
impl Checkpoint {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::OldMoved => "old-moved",
            Self::NewMoved => "new-moved",
            Self::LockWritten => "lock-written",
            Self::ReceiptWritten => "receipt-written",
            Self::Committed => "committed",
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Recovery {
    pub instance_id: String,
    pub transaction: String,
    pub action: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    transaction: String,
    instance_id: String,
    had_mods: bool,
    had_lock: bool,
    had_receipt: bool,
    new_lock_sha512: String,
}

pub struct Workspace {
    root: PathBuf,
    _lease: File,
}
impl Workspace {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        let root = fs::canonicalize(root)?;
        let lock_path = root.join(".host.lock");
        ensure_regular_or_absent(&lock_path)?;
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        lease.try_lock_exclusive().map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.raw_os_error() == fs2::lock_contended_error().raw_os_error()
            {
                Error::Busy
            } else {
                Error::Io(e)
            }
        })?;
        let workspace = Self {
            root,
            _lease: lease,
        };
        for dir in [
            "store",
            "store/sha512",
            "instances",
            "downloads",
            "retired-transactions",
            "new-instances",
        ] {
            workspace.dir(dir)?;
        }
        Ok(workspace)
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    fn dir(&self, relative: &str) -> Result<PathBuf> {
        let path = self.root.join(relative);
        checked_dir(&self.root, &path)?;
        Ok(path)
    }
    pub fn instance_dir(&self, id: &str) -> Result<PathBuf> {
        validate_uuid(id)?;
        let path = self.root.join("instances").join(id);
        checked_existing_dirs(&self.root, &path)?;
        Ok(path)
    }
    pub fn trash_dir(&self) -> Result<PathBuf> {
        self.dir("trash")
    }
    pub fn trash_instance_dir(&self, id: &str) -> Result<PathBuf> {
        validate_uuid(id)?;
        let path = self.trash_dir()?.join(id);
        checked_existing_dirs(&self.root, &path)?;
        Ok(path)
    }
    pub fn archive_instance(&self, id: &str) -> Result<()> {
        self.instance(id)?;
        let from = self.instance_dir(id)?;
        let to = self.trash_instance_dir(id)?;
        if to.exists() {
            return Err(Error::Invalid("Instance already in trash".into()));
        }
        fs::rename(from, to)?;
        Ok(())
    }
    pub fn pending_deletion_ids(&self) -> Result<Vec<String>> {
        let mut ids=Vec::new();
        for entry in fs::read_dir(self.dir("deleting")?)? {
            let id=entry?.file_name().to_string_lossy().into_owned();
            validate_uuid(&id)?;
            ids.push(id);
        }
        Ok(ids)
    }
    pub fn begin_permanent_delete(&self,id:&str)->Result<()> {
        self.instance(id)?;
        let from=self.instance_dir(id)?;
        let to=self.dir("deleting")?.join(id);
        checked_existing_dirs(&self.root,&to)?;
        if to.exists(){return Err(Error::Invalid("Deletion already pending".into()))}
        fs::rename(from,to)?;
        Ok(())
    }
    pub fn finish_permanent_delete(&self,id:&str)->Result<()> {
        validate_uuid(id)?;
        let path=self.dir("deleting")?.join(id);
        checked_existing_dirs(&self.root,&path)?;
        if path.exists(){remove_tree(&path)?;}
        Ok(())
    }
    pub fn restore_instance(&self, id: &str) -> Result<()> {
        let from = self.trash_instance_dir(id)?;
        let to = self.instance_dir(id)?;
        let i: Instance = read_json(&from.join("instance.json"))?;
        i.validate()?;
        if i.instance_id != id || to.exists() {
            return Err(Error::Invalid("Restore identity conflict".into()));
        }
        fs::rename(from, to)?;
        Ok(())
    }
    pub fn create_instance(&self, instance: &Instance) -> Result<()> {
        instance.validate()?;
        let dir = self.instance_dir(&instance.instance_id)?;
        if dir.exists() {
            return Err(Error::Invalid("Instance already exists".into()));
        }
        // Publish a complete directory, never a half-written instance config.
        let stage = self
            .root
            .join("new-instances")
            .join(Uuid::new_v4().to_string());
        checked_dir(&self.root, &stage)?;
        checked_dir(&self.root, &stage.join("game"))?;
        checked_dir(&self.root, &stage.join("transactions"))?;
        write_new(&stage.join("instance.json"), &json_bytes(instance)?)?;
        sync_dir(&stage)?;
        fs::rename(&stage, &dir)?;
        sync_dir(dir.parent().unwrap())?;
        Ok(())
    }
    pub fn instance(&self, id: &str) -> Result<Instance> {
        let instance: Instance = read_json(&self.instance_dir(id)?.join("instance.json"))?;
        instance.validate()?;
        if instance.instance_id != id {
            return Err(Error::Invalid("Instance ID mismatch".into()));
        }
        Ok(instance)
    }
    pub fn instances(&self) -> Result<Vec<Instance>> {
        let mut instances = Vec::new();
        for entry in fs::read_dir(self.root.join("instances"))? {
            let entry = entry?;
            ensure_directory(&entry.path())?;
            let id = entry.file_name().to_string_lossy().into_owned();
            validate_uuid(&id)?;
            instances.push(self.instance(&id)?);
        }
        instances.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(instances)
    }
    pub fn blob_path(&self, hash: &str) -> Result<PathBuf> {
        validate_hash(hash)?;
        let bucket = self.root.join("store/sha512").join(&hash[..2]);
        checked_existing_dirs(&self.root, &bucket)?;
        let file = bucket.join(hash);
        ensure_regular_or_absent(&file)?;
        Ok(file)
    }
    pub fn import_jar(&self, source: impl AsRef<Path>) -> Result<Blob> {
        let source = source.as_ref();
        let mut input = File::open(source)?;
        let length = input.metadata()?.len();
        if !input.metadata()?.is_file() || !(4..=MAX_BLOB).contains(&length) {
            return Err(Error::Invalid(
                "Expected a JAR file between 4 bytes and 1 GiB".into(),
            ));
        }
        let tmp = self
            .root
            .join("downloads")
            .join(format!("{}.part", Uuid::new_v4()));
        let result = (|| {
            let mut output = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
            let mut hasher = Sha512::new();
            let mut total = 0u64;
            let mut buffer = [0u8; 65536];
            let mut prefix = Vec::new();
            loop {
                let n = input.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                if prefix.len() < 4 {
                    prefix.extend_from_slice(&buffer[..n.min(4 - prefix.len())]);
                }
                total += n as u64;
                if total > MAX_BLOB {
                    return Err(Error::Invalid("JAR exceeds size limit".into()));
                }
                hasher.update(&buffer[..n]);
                output.write_all(&buffer[..n])?;
            }
            if !matches!(prefix.as_slice(), b"PK\x03\x04" | b"PK\x05\x06") {
                return Err(Error::Invalid("Missing JAR/ZIP signature".into()));
            }
            output.sync_all()?;
            drop(output);
            let hash = format!("{:x}", hasher.finalize());
            let dest = self.blob_path(&hash)?;
            checked_dir(&self.root, dest.parent().unwrap())?;
            let reused = dest.exists() && digest_file(&dest)? == (hash.clone(), total);
            if !reused {
                // rename replaces the directory entry, never overwrites a shared inode.
                fs::rename(&tmp, &dest)?;
                sync_dir(dest.parent().unwrap())?;
            }
            Ok(Blob {
                sha512: hash,
                bytes: total,
                reused,
            })
        })();
        if tmp.exists() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
    pub fn verify_blob(&self, hash: &str, bytes: u64) -> Result<()> {
        let p = self.blob_path(hash)?;
        let actual = digest_file(&p)?;
        if actual != (hash.to_owned(), bytes) {
            return Err(Error::HashMismatch(hash.into()));
        }
        Ok(())
    }
    pub fn read_lock(&self, id: &str) -> Result<Option<Lockfile>> {
        let path = self.instance_dir(id)?.join("blocklink.lock.json");
        ensure_regular_or_absent(&path)?;
        if !path.exists() {
            return Ok(None);
        }
        let lock: Lockfile = read_json(&path)?;
        lock.validate()?;
        Ok(Some(lock))
    }
    pub fn verify_instance(&self, id: &str) -> Result<Receipt> {
        self.ensure_no_transaction(id)?;
        let dir = self.instance_dir(id)?;
        let lock = self
            .read_lock(id)?
            .ok_or_else(|| Error::Invalid("Instance has no applied lockfile".into()))?;
        self.instance(id)?.accepts(&lock)?;
        let receipt: Receipt = read_json(&dir.join("receipt.json"))?;
        if receipt.schema_version != 1
            || digest_file(&dir.join("blocklink.lock.json"))?.0 != receipt.lock_sha512
        {
            return Err(Error::HashMismatch("lockfile/receipt".into()));
        }
        let mods = dir.join("game/mods");
        ensure_directory(&mods)?;
        for m in &lock.mods {
            let installed = mods.join(&m.file);
            ensure_regular_or_absent(&installed)?;
            if digest_file(&installed)? != (m.sha512.clone(), m.bytes) {
                return Err(Error::HashMismatch(m.file.clone()));
            }
            self.verify_blob(&m.sha512, m.bytes)?;
        }
        Ok(receipt)
    }
    fn ensure_no_transaction(&self, id: &str) -> Result<()> {
        let path = self.instance_dir(id)?.join("transactions");
        checked_existing_dirs(&self.root, &path)?;
        if path.exists() && fs::read_dir(&path)?.next().is_some() {
            return Err(Error::RecoveryRequired(id.into()));
        }
        Ok(())
    }
    pub fn sync(&self, id: &str, lock: &Lockfile, mode: LinkMode) -> Result<Receipt> {
        self.sync_with_checkpoints(id, lock, mode, |_| {})
    }
    /// Checkpoint notifications support progress tracking. Tests terminate subprocesses here.
    pub fn sync_with_checkpoints(
        &self,
        id: &str,
        lock: &Lockfile,
        mode: LinkMode,
        mut checkpoint: impl FnMut(Checkpoint),
    ) -> Result<Receipt> {
        self.ensure_no_transaction(id)?;
        let instance = self.instance(id)?;
        instance.accepts(lock)?;
        let dir = self.instance_dir(id)?;
        let mods = dir.join("game/mods");
        checked_existing_dirs(&self.root, &dir.join("game"))?;
        if mods.exists() {
            ensure_directory(&mods)?;
        } else {
            ensure_directory_or_absent(&mods)?;
        }
        for m in &lock.mods {
            self.verify_blob(&m.sha512, m.bytes)?;
        }
        let prior = self.read_lock(id)?;
        let managed: HashSet<String> = prior
            .as_ref()
            .map(|l| l.mods.iter().map(|m| filename_key(&m.file)).collect())
            .unwrap_or_default();
        let wanted: HashSet<String> = lock.mods.iter().map(|m| filename_key(&m.file)).collect();
        let transaction = Uuid::new_v4().to_string();
        let tx = dir.join("transactions").join(&transaction);
        checked_dir(&self.root, &tx)?;
        let stage = tx.join("new-mods");
        checked_dir(&self.root, &stage)?;
        let bytes = json_bytes(lock)?;
        let mut receipt = Receipt {
            schema_version: 1,
            generation: transaction.clone(),
            lock_sha512: digest(&bytes),
            ..Default::default()
        };
        let prepare = (|| {
            if mods.exists() {
                for entry in fs::read_dir(&mods)? {
                    let entry = entry?;
                    let filename = entry.file_name();
                    let text = filename
                        .to_str()
                        .ok_or_else(|| Error::UnsafePath("Non-UTF8 Mod filename".into()))?;
                    if managed.contains(&filename_key(text)) {
                        ensure_regular_or_absent(&entry.path())?;
                        continue;
                    }
                    if wanted.contains(&filename_key(text)) {
                        return Err(Error::Collision(text.into()));
                    }
                    copy_tree(&entry.path(), &stage.join(filename), 0)?;
                    receipt.preserved += 1;
                }
            }
            for m in &lock.mods {
                let source = self.blob_path(&m.sha512)?;
                let target = stage.join(&m.file);
                if mode == LinkMode::Copy {
                    copy_file(&source, &target)?;
                    receipt.copied += 1;
                    continue;
                }
                if mode == LinkMode::Auto && try_clone(&source, &target)? {
                    receipt.cloned += 1;
                    continue;
                }
                match fs::hard_link(&source, &target) {
                    Ok(()) => receipt.linked += 1,
                    Err(e) => {
                        if target.exists() {
                            return Err(Error::Collision(m.file.clone()));
                        }
                        copy_file(&source, &target)?;
                        receipt.copied += 1;
                        receipt
                            .fallback_reasons
                            .push(format!("{}: hardlink unavailable ({})", m.file, e));
                    }
                }
            }
            // Recheck staged bytes, including clones/copies, before touching active Mods.
            for m in &lock.mods {
                if digest_file(&stage.join(&m.file))? != (m.sha512.clone(), m.bytes) {
                    return Err(Error::HashMismatch(m.file.clone()));
                }
            }
            let had_lock = dir.join("blocklink.lock.json").exists();
            let had_receipt = dir.join("receipt.json").exists();
            if had_lock {
                copy_file(&dir.join("blocklink.lock.json"), &tx.join("old-lock.json"))?;
            }
            if had_receipt {
                copy_file(&dir.join("receipt.json"), &tx.join("old-receipt.json"))?;
            }
            write_new(&tx.join("new-lock.json"), &bytes)?;
            write_new(&tx.join("new-receipt.json"), &json_bytes(&receipt)?)?;
            sync_dir(&stage)?;
            let journal = Journal {
                schema_version: 1,
                transaction: transaction.clone(),
                instance_id: id.into(),
                had_mods: mods.exists(),
                had_lock,
                had_receipt,
                new_lock_sha512: receipt.lock_sha512.clone(),
            };
            write_new(&tx.join("journal.pending"), &json_bytes(&journal)?)?;
            fs::rename(tx.join("journal.pending"), tx.join("journal.json"))?;
            sync_dir(&tx)?;
            sync_dir(tx.parent().unwrap())?;
            Ok(())
        })();
        if let Err(e) = prepare {
            remove_tree(&tx)?;
            return Err(e);
        }
        checkpoint(Checkpoint::Prepared);
        let commit = (|| {
            if mods.exists() {
                fs::rename(&mods, tx.join("old-mods"))?;
                sync_dir(mods.parent().unwrap())?;
                sync_dir(&tx)?;
            }
            checkpoint(Checkpoint::OldMoved);
            fs::rename(&stage, &mods)?;
            sync_dir(mods.parent().unwrap())?;
            sync_dir(&tx)?;
            checkpoint(Checkpoint::NewMoved);
            replace_file(&tx.join("new-lock.json"), &dir.join("blocklink.lock.json"))?;
            checkpoint(Checkpoint::LockWritten);
            replace_file(&tx.join("new-receipt.json"), &dir.join("receipt.json"))?;
            checkpoint(Checkpoint::ReceiptWritten);
            write_new(&tx.join("committed"), b"1")?;
            sync_dir(&tx)?;
            checkpoint(Checkpoint::Committed);
            Ok(())
        })();
        if let Err(e) = commit {
            self.recover_transaction(id, &transaction)
                .map_err(|r| Error::RecoveryRequired(format!("{}; recovery failed: {}", e, r)))?;
            return Err(e);
        }
        self.retire_transaction(&tx)?;
        Ok(receipt)
    }
    pub fn recover_all(&self) -> Result<Vec<Recovery>> {
        // Neither namespace can contain an active transaction or published instance.
        for namespace in ["retired-transactions", "new-instances"] {
            let directory = self.root.join(namespace);
            ensure_directory(&directory)?;
            for entry in fs::read_dir(&directory)? {
                let entry = entry?;
                validate_uuid(&entry.file_name().to_string_lossy())?;
                remove_tree(&entry.path())?;
            }
            sync_dir(&directory)?;
        }
        let mut results = Vec::new();
        for instance in self.instances()? {
            let txs = self
                .instance_dir(&instance.instance_id)?
                .join("transactions");
            checked_existing_dirs(&self.root, &txs)?;
            if !txs.exists() {
                continue;
            }
            for entry in fs::read_dir(&txs)? {
                let entry = entry?;
                ensure_directory(&entry.path())?;
                let id = entry.file_name().to_string_lossy().into_owned();
                validate_uuid(&id)?;
                results.push(self.recover_transaction(&instance.instance_id, &id)?);
            }
        }
        Ok(results)
    }
    fn recover_transaction(&self, id: &str, transaction: &str) -> Result<Recovery> {
        validate_uuid(transaction)?;
        let dir = self.instance_dir(id)?;
        let tx = dir.join("transactions").join(transaction);
        checked_existing_dirs(&self.root, &tx)?;
        let journal_path = tx.join("journal.json");
        ensure_regular_or_absent(&journal_path)?;
        if !journal_path.exists() {
            remove_tree(&tx)?;
            return Ok(Recovery {
                instance_id: id.into(),
                transaction: transaction.into(),
                action: "discarded-incomplete-preparation".into(),
            });
        }
        let journal: Journal = read_json(&journal_path)?;
        if journal.schema_version != 1
            || journal.instance_id != id
            || journal.transaction != transaction
        {
            return Err(Error::RecoveryRequired("Journal identity mismatch".into()));
        }
        validate_hash(&journal.new_lock_sha512)?;
        let mods = dir.join("game/mods");
        checked_existing_dirs(&self.root, mods.parent().unwrap())?;
        ensure_regular_or_absent(&tx.join("committed"))?;
        let action = if tx.join("committed").exists() {
            let lock: Lockfile = read_json(&dir.join("blocklink.lock.json"))?;
            lock.validate()?;
            if digest_file(&dir.join("blocklink.lock.json"))?.0 != journal.new_lock_sha512 {
                return Err(Error::RecoveryRequired("Committed lockfile changed".into()));
            }
            let receipt: Receipt = read_json(&dir.join("receipt.json"))?;
            if receipt.generation != transaction || receipt.lock_sha512 != journal.new_lock_sha512 {
                return Err(Error::RecoveryRequired("Committed receipt changed".into()));
            }
            ensure_directory(&mods)?;
            for m in &lock.mods {
                if digest_file(&mods.join(&m.file))? != (m.sha512.clone(), m.bytes) {
                    return Err(Error::RecoveryRequired("Committed Mod changed".into()));
                }
            }
            "completed-committed-transaction"
        } else {
            let backup = tx.join("old-mods");
            ensure_directory_or_absent(&backup)?;
            if backup.exists() {
                if mods.exists() {
                    remove_tree(&mods)?;
                }
                fs::rename(&backup, &mods)?;
                sync_dir(mods.parent().unwrap())?;
            } else if !journal.had_mods && mods.exists() {
                remove_tree(&mods)?;
                sync_dir(mods.parent().unwrap())?;
            } else if journal.had_mods && !mods.exists() {
                return Err(Error::RecoveryRequired(
                    "Both old and active Mods directories are missing".into(),
                ));
            }
            restore_snapshot(
                &tx.join("old-lock.json"),
                &dir.join("blocklink.lock.json"),
                journal.had_lock,
            )?;
            restore_snapshot(
                &tx.join("old-receipt.json"),
                &dir.join("receipt.json"),
                journal.had_receipt,
            )?;
            "rolled-back"
        };
        self.retire_transaction(&tx)?;
        Ok(Recovery {
            instance_id: id.into(),
            transaction: transaction.into(),
            action: action.into(),
        })
    }
    fn retire_transaction(&self, tx: &Path) -> Result<()> {
        // Remove from the recovery namespace atomically before deleting any marker
        // or snapshot. A crash during cleanup must never turn a commit into rollback.
        let dest = self
            .root
            .join("retired-transactions")
            .join(Uuid::new_v4().to_string());
        fs::rename(tx, &dest)?;
        sync_dir(tx.parent().unwrap())?;
        sync_dir(dest.parent().unwrap())?;
        remove_tree(&dest)?;
        Ok(())
    }
}

pub fn read_lockfile(path: impl AsRef<Path>) -> Result<Lockfile> {
    let l: Lockfile = read_json(path.as_ref())?;
    l.validate()?;
    Ok(l)
}
pub fn read_instance(path: impl AsRef<Path>) -> Result<Instance> {
    let i: Instance = read_json(path.as_ref())?;
    i.validate()?;
    Ok(i)
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha512::digest(bytes))
}
fn digest_file(path: &Path) -> Result<(String, u64)> {
    ensure_regular_or_absent(path)?;
    let mut f = File::open(path)?;
    let mut hash = Sha512::new();
    let mut bytes = 0;
    let mut buffer = [0; 65536];
    loop {
        let n = f.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        bytes += n as u64;
        if bytes > MAX_BLOB {
            return Err(Error::Invalid("File exceeds limit".into()));
        }
    }
    Ok((format!("{:x}", hash.finalize()), bytes))
}
fn json_bytes(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_JSON {
        return Err(Error::Invalid("JSON exceeds limit".into()));
    }
    Ok(bytes)
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    ensure_regular_or_absent(path)?;
    let mut data = Vec::new();
    File::open(path)?
        .take(MAX_JSON + 1)
        .read_to_end(&mut data)?;
    if data.len() as u64 > MAX_JSON {
        return Err(Error::Invalid("JSON exceeds limit".into()));
    }
    Ok(serde_json::from_slice(&data)?)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}
fn copy_file(from: &Path, to: &Path) -> Result<()> {
    ensure_regular_or_absent(from)?;
    let mut source = File::open(from)?;
    let mut target = OpenOptions::new().write(true).create_new(true).open(to)?;
    std::io::copy(&mut source, &mut target)?;
    target.sync_all()?;
    Ok(())
}
fn try_clone(from: &Path, to: &Path) -> Result<bool> {
    #[cfg(unix)]
    {
        let temp = to.with_extension(format!("clone-{}", Uuid::new_v4()));
        match reflink_copy::reflink(from, &temp) {
            Ok(()) => {
                File::open(&temp)?.sync_all()?;
                fs::rename(&temp, to)?;
                Ok(true)
            }
            Err(_) => {
                ensure_regular_or_absent(&temp)?;
                if temp.exists() {
                    fs::remove_file(&temp)?;
                }
                Ok(false)
            }
        }
    }
    // Windows currently uses tested hardlinks/copy. ReFS cloning needs its own
    // integration coverage before enabling the upstream experimental path.
    #[cfg(not(unix))]
    {
        let _ = (from, to);
        Ok(false)
    }
}
fn replace_file(from: &Path, to: &Path) -> Result<()> {
    ensure_regular_or_absent(from)?;
    ensure_regular_or_absent(to)?;
    fs::rename(from, to)?;
    sync_dir(to.parent().unwrap())?;
    Ok(())
}
fn restore_snapshot(from: &Path, to: &Path, existed: bool) -> Result<()> {
    ensure_regular_or_absent(to)?;
    if existed {
        ensure_regular_or_absent(from)?;
        let bytes = fs::read(from)?;
        let temp = to.with_extension(format!("recover-{}", Uuid::new_v4()));
        write_new(&temp, &bytes)?;
        replace_file(&temp, to)?;
    } else if to.exists() {
        fs::remove_file(to)?;
        sync_dir(to.parent().unwrap())?;
    }
    Ok(())
}
fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}
fn ensure_regular_or_absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if !m.is_file() || reparse(&m) => Err(Error::UnsafePath(path.display().to_string())),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
fn ensure_directory(path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir() || reparse(&m) {
        return Err(Error::UnsafePath(path.display().to_string()));
    }
    Ok(())
}
fn ensure_directory_or_absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if !m.is_dir() || reparse(&m) => Err(Error::UnsafePath(path.display().to_string())),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
fn checked_existing_dirs(root: &Path, path: &Path) -> Result<()> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| Error::UnsafePath(path.display().to_string()))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(Error::UnsafePath(path.display().to_string()));
        }
        current.push(component);
        ensure_directory_or_absent(&current)?;
    }
    Ok(())
}
fn checked_dir(root: &Path, path: &Path) -> Result<()> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| Error::UnsafePath(path.display().to_string()))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(Error::UnsafePath(path.display().to_string()));
        }
        current.push(component);
        ensure_directory_or_absent(&current)?;
        if !current.exists() {
            fs::create_dir(&current)?;
        }
    }
    Ok(())
}
fn copy_tree(from: &Path, to: &Path, depth: u32) -> Result<()> {
    if depth > 32 {
        return Err(Error::Invalid(
            "Unmanaged directory nesting exceeds limit".into(),
        ));
    }
    let metadata = fs::symlink_metadata(from)?;
    if reparse(&metadata) {
        return Err(Error::UnsafePath(from.display().to_string()));
    }
    if metadata.is_file() {
        copy_file(from, to)?;
    } else if metadata.is_dir() {
        fs::create_dir(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()), depth + 1)?;
        }
        sync_dir(to)?;
    } else {
        return Err(Error::UnsafePath(from.display().to_string()));
    }
    Ok(())
}
fn remove_tree(path: &Path) -> Result<()> {
    ensure_directory(path)?;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if reparse(&metadata) {
            return Err(Error::UnsafePath(entry.path().display().to_string()));
        }
        if metadata.is_dir() {
            remove_tree(&entry.path())?;
        } else if metadata.is_file() {
            fs::remove_file(entry.path())?;
        } else {
            return Err(Error::UnsafePath(entry.path().display().to_string()));
        }
    }
    fs::remove_dir(path)?;
    Ok(())
}
fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()?;
    }
    // Windows std does not offer directory FlushFileBuffers. Files are flushed;
    // abrupt-process recovery is supported, power-loss ordering is not promised.
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
