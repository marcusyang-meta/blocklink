# Personal game home

- The default client instance is selected using local successful process-start timestamps and world LastPlayed values. Manual selection is preserved during background refreshes.
- last-played.json is written only after the game process starts. Login alone, cancellation and failed preparation do not count. A process start does not prove entry into a world, so this is labeled Last launched.
- Recent world names and play times come from actual Java saves; covers come from each world's icon.png. Missing worlds do not produce invented activity.
- The home screen uses a bundled scene background. Personal world pictures are used only as small covers, remain local, and are not automatically uploaded or shared. The local screenshot API remains available but is not used by the home screen.
- Optional information refreshes every 15 seconds. Play starts the game without automatically opening or rewriting a world. Recent activity is local, not a cloud activity feed or friends' online status.

Historical checks: 25 service tests passed, with seven existing external integration tests skipped. Checks included persistent history, image size limits, path boundaries and missing-image fallbacks. TypeScript and Vite passed. Isolated worlds verified NBT timestamps and 64-by-64 covers; the Play button remained visible at 920-by-587 without horizontal overflow. User worlds were not launched or modified.


[Chinese version](HOME-PERSONALIZATION.zh-CN.md)
