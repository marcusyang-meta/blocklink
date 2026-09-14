# Permanently deleting games and servers

Delete a game or server from its details page, enter its full displayed name, and confirm permanent deletion. This removes game files, worlds, configuration, shaders, logs and local world backups without moving them to a recycle bin. Other games and shared download caches remain.

Deletion is rejected while the instance or its bound client is running, or a world is locked by an external game. Directory links and paths outside the managed root are rejected. Rooms are closed and local bindings removed.

After confirmation, the directory is moved out of the library before its files are removed. This internal staging area is not a recoverable recycle bin. Cleanup resumes after an interrupted process; files held by another program require that program to close first.

The navigation no longer exposes a recycle bin. Data placed in the old recycle bin is not automatically purged; legacy restore endpoints remain for compatibility.

Tests cover name confirmation, world locks, deletion of saves and backups, preservation of shared hard-linked content, absence of new recycle-bin entries, and cleanup after interruption.


[Chinese version](DELETION.zh-CN.md)
