# Mechanics sheet

Values read from the original program (decompiled with Ghidra, strings from its data sections).
The rebuild's code uses these values exactly as written here.

## Main window

| Item | Value | Source |
|---|---|---|
| Toolbar buttons | Add URL · Resume · Stop · Stop All · Delete · Options · Scheduler · Start Queue · Stop Queue · Grabber · Tell a Friend | strings 0x6b9214…0x6b91ac, string map `sub_4ec900` (0x4edd44–0x4eddda) |
| List columns | File Name · Size · Status · Time left · Transfer rate · Last Try Date · Description | strings 0x6ba050, 0x6a804c, 0x6a9380, 0x6b922c, 0x6b921c, 0x6b8a04, 0x6a8034 |
| Category tree | All Downloads → Compressed, Documents, Music, Programs, Video; Unfinished; Finished; Grabber projects; Queues (Main download queue, Synchronization queue) | strings 0x6ba02c–0x6b9fe4, 0x6b31fc, 0x6b33f8, 0x6b33e0, 0x6b5fa4 |

## Categories and file types

Default extension lists (function `sub_4c94f0`, 0x4c97c4…0x4c9b03). A file goes to the first category whose list holds its extension; `r0*`/`r1*` are wildcards.

| Category | Extensions |
|---|---|
| Compressed | zip rar r0* r1* arj gz sit sitx sea ace bz2 7z |
| Documents | doc pdf ppt pps docx pptx |
| Music | mp3 wav wma mpa ram ra aac aif m4a tsa |
| Programs | exe msi |
| Video | avi mpg mpe mpeg asf wmv mov qt rm mp4 flv m4v webm ogv ogg mkv ts tsv |

Save folder per category: `<user folder>\Downloads\<Category>` (format `%s\Downloads\%s` at 0x69e30c; category name join in `sub_4c8260`). Category ids: Programs 1, Music 2, Video 3, Documents 5, Compressed 7 (`sub_4c8260`). Files with no category go to `<user folder>\Downloads`.

## Downloading

| Rule | Value | Source |
|---|---|---|
| Part takeover | When a connection finishes its part and the connection right after it has received fewer than **50 bytes**, the finished connection takes over that part and the other is closed | `sub_5a4df0` (`piVar4[0x20] < 0x32`) |
| Saved speed | The last speed is stored with a download only when it is above **100** bytes/s | `sub_5ba210` |
| Speed limiter | Per-download on/off flag + limit value, kept on stop/resume | `sub_5b9dd0` (`bUseSpeedLimiter`, `psnSpeedLimit`) |
| Time left text | `%d sec` under a minute, `%d min %d sec` otherwise | strings 0x6b8bb0, 0x6b8bb8 |
| Status texts | Receiving data... · Complete · Error · Connection failed. | 0x6bbb20, 0x6b8c38, 0x6a84b0, 0x6bb7ec |
| Default connections per download | **8** — estimated (not found in code; the combo values are filled from code not yet located) | estimated |
| Connection choices | 1, 2, 4, 8, 16, 24, 32 — estimated | estimated |
| Max downloads at once in a queue | 4 — estimated | estimated |
| Size units | KB / MB / GB with 2 decimals — estimated (unit strings live in the string-table resource) | estimated |

## How the rebuild splits a file (own design from the rules above)

1. Ask the server for the size and whether it accepts ranges (one request with `Range: bytes=0-`; a 206 answer with `Content-Range` gives both).
2. No size or no range support ⇒ one connection from the start (matches the "server does not support resume" message, 0x6b2db0).
3. Otherwise split into N equal parts (N = connections). Each part writes straight into the target file at its offset.
4. When a part finishes, the biggest remaining part is cut in half and a new connection takes the second half (dynamic segmentation); a part with fewer than 50 bytes left is not split (the 50-byte rule).
5. A connection that the server refuses (for example "429 too many requests") gives its part back; the next free connection picks it up first. The download fails only when the last connection fails.
6. Stop keeps the received ranges; Resume continues each unfinished part from where it stopped.

| Own-design value | Value | Source |
|---|---|---|
| Smallest half a part may be cut into | 256 KB | estimated |
| Retries per connection before giving up | 5, 2 s apart | estimated |
| Clipboard watching on by default | yes | estimated (option text at 0x799900) |
