# Game menu interface

The home screen uses a full-window block-world background, top navigation, horizontal instance selection and a bottom launch area. Selecting a game updates its name and version beside a single primary Play button. Running games show Stop. Preparation and account flows retain their existing behavior.

Mods, Worlds & saves, and Shaders are directly accessible. Runtime details, logs and update recovery live in advanced management. Shared storage is accessed through Launcher settings and shows mod storage use and the data-folder entry point. File deduplication is unchanged. The current deletion flow is permanent; see [deletion behavior](DELETION.md).

The background is a bundled generated asset, and default instance thumbnails use different crops of that asset rather than pretending to be save screenshots. See [asset notes](desktop/src/assets/README.md).

Validation included TypeScript and Vite, layouts at 1240-by-787 and 920-by-587, selected-instance state, search and clearing, creation and invitation entry points, and real shader/world/backup lists from an isolated backend. The selected NeoForge test instance opened the player-profile flow without downloading or launching that test game. User game content was not changed.

Buttons and inputs retain keyboard focus styling, selections use aria-pressed, and reduced-motion preferences are respected. This was not a full accessibility compliance audit. Microsoft API access remains unapproved.


[Chinese version](UI-REDESIGN.zh-CN.md)
