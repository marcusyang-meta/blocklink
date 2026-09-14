# Microsoft sign-in integration

Minecraft API access is not approved. Implemented Microsoft authorization must not be presented as successful authenticated Minecraft login.

Blocklink's public client ID is `a8080fd5-fcd6-4dbf-811f-91d0f10759c9`. The application supports personal Microsoft accounts and public client flows. Device-code authorization, Xbox/XSTS exchange, Minecraft profile lookup, OS credential storage and token refresh are implemented. Minecraft access has returned a refusal; a 403 alone does not identify every possible underlying cause.

Fork publishers should register their own public client and set `BLOCKLINK_MICROSOFT_CLIENT_ID` before building. Blocklink uses the consumers device-code endpoint with `XboxLive.signin offline_access`; desktop builds do not need a client secret. Empty clientId settings use the built-in UUID, while existing custom settings are preserved. An override is also available in advanced launcher settings.

End-to-end acceptance requires actual device authorization, Xbox/XSTS exchange, a Minecraft profile, game startup and refresh. Cancellation, expiration, application configuration errors, Minecraft access refusal and missing profiles have separate readable errors without exposing authorization credentials.

The UI uses the returned verification_uri, including microsoft.com/link where supplied, rather than hard-coding microsoft.com/devicelogin.

References: [Microsoft app registration](https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app), [Minecraft API application review](https://aka.ms/mce-reviewappid), and [launcher integration documentation](https://minecraft-launcher-lib.readthedocs.io/en/stable/tutorial/microsoft_login.html). Registration and Microsoft sign-in alone do not establish Minecraft API approval; no review deadline or approval is promised.


[Chinese version](MICROSOFT-SETUP.zh-CN.md)
