# Rallx Cheat Launcher

A portable Windows app for organizing game trainers, launching them with a shortcut, and sending your saved cheat key combinations. Designed for mouse, touch, and gamepad use on PCs and Windows handhelds such as the ROG Ally.

**[Download Rallx Cheat Launcher → GitHub Releases](https://github.com/rajamuthukumaran-d/rallx-cheat-launcher/releases)**

## Download and set up

1. Open the [releases page](https://github.com/rajamuthukumaran-d/rallx-cheat-launcher/releases), choose a release, and download the Windows app from **Assets**. The source-code archives are for developers.
2. If the download is an archive, extract it before running the app. Keep the app in a folder you can write to, such as `C:\Rallx`.
3. Run `rallx-cheat-launcher.exe`. **Run as administrator** is enabled by default; approve the Windows UAC prompt to start elevated.
4. Open **Settings → Trainer folder → Browse…** and select the folder containing your trainer `.exe` files.
5. Return to Home. Use **Refresh** after adding files to the folder outside the app.

Rallx launches trainers you supply; it does not download trainers or include cheats. You do not need Rust or Cargo to use a downloaded build. The app targets Windows; SteamOS is not a native target.

## Add and configure a trainer

Use the **Add** icon on Home or drop a trainer `.exe` onto the app. Choose the executable, give it a name, and save it. **Importing moves the executable into the selected trainer folder.** Deleting a trainer after confirmation also deletes its executable from that folder.

Click a trainer row to open **Edit**. The row's **Play** icon launches it; the rest of the row opens its settings. The Edit header also provides launch, copy-launch-script, and delete actions.

For each trainer, you can configure:

| Setting | What it does |
| --- | --- |
| Shortcut to launch trainer | Sets the hotkey used by a single trainer mode launch script. Normal mode uses the global shortcut in Settings. |
| Cheats to enable | Stores the trainer's activation keys and cheat combinations. Use **Add cheat** and the key recorder to capture the keys the trainer expects. |
| Auto trigger cheat on launch | Sends the saved cheat sequence after the trainer is ready and the configured delay has elapsed. Off by default; the default delay is 3 seconds. |
| Game/app executable to watch | Matches the running game for normal-mode hotkeys and closes the launched trainer when that game exits. |
| Game executable / Game launch options | Configures the game used by **Generate .bat** in the **Generate Launch script** section. Selecting a Windows shortcut (`.lnk`) fills its target and arguments. |

Save the trainer after editing its settings. Cheat keys must match the trainer's own controls, including any activation key it requires.

## Launch a trainer and activate cheats

1. Start your game.
2. Click the trainer's **Play** icon, use **Play** in Edit, or select it with a gamepad and press **A**.
3. With **Auto trigger cheat on launch** off, the first action opens the trainer without sending cheats. Trigger it again once it is ready to send the saved cheat sequence.
4. With auto-trigger on, Rallx waits for readiness and the configured delay, then sends the sequence automatically. Increase the delay if the trainer takes longer to initialize.

Further launch actions reuse the trainer process Rallx is tracking instead of opening duplicates. Sending a trainer's toggle key again may disable that cheat; the trainer determines what each key does.

## Use the global shortcut while gaming

In normal mode, Rallx can pick the trainer for the game you are playing:

1. Edit the trainer and set **Game/app executable to watch** to the game's actual running executable.
2. Set **Settings → Default launch shortcut**. The initial shortcut is **Ctrl+Alt+Shift+F3**.
3. Start the game and leave Rallx running, either with its window visible or in the system tray.
4. Press the global shortcut to launch the matching trainer. Cheat activation follows the auto-trigger setting described above.

A fresh hotkey launch that sends default cheats minimizes the trainer afterward. When the watched game exits, Rallx closes the trainer it launched. Rallx stays running in normal mode.

## Background startup and closing behavior

| Setting | Behavior |
| --- | --- |
| Run in background | Starts normal mode hidden in the system tray. Minimizing returns it to the tray, and the global shortcut stays active. Off by default. |
| Start on login | Starts normal mode in the tray when you sign in to Windows. Automatically enables and locks **Run in background** until Start on login is disabled. Off by default. |
| Close app after launching trainer | Exits Rallx after the launch sequence. Disables **Run in background** and **Start on login**, with confirmation when either is active. Off by default. |
| Confirm before closing app | Asks before closing through the title bar, keyboard, or gamepad. On by default. |
| Run as administrator | Applies on the next startup. Use **Restart as admin** to apply immediately; it is disabled when already elevated. On by default. |

Click the tray icon or choose **Open Rallx** in its menu to restore the window. Choose **Exit** to quit. With administrator mode enabled, Start on login uses an elevated Windows logon task with a brief delay for the tray to become ready.

**Close-after-launch takes priority over watched-game cleanup:** Rallx exits and leaves the trainer running. If auto-trigger is on, it finishes the cheat sequence before exiting. If auto-trigger is off, it waits for the trainer to open, minimizes it, skips cheats, and exits.

The global close-after-launch setting controls launches from the normal app. A trainer's saved close-after-launch launch-script option only controls the generated script.

## Single trainer mode and launch scripts

Single trainer mode is a tray-only process dedicated to one trainer. Start it with `--launch`; it waits for its resolved hotkey before launching the trainer.

The easiest way to get the command is **Copy launch script** from the trainer's Edit header, or **RB** on Home. To create a game launch batch file, expand **Generate Launch script**, select the game and any launch options, and choose **Generate .bat**.

For a Windows shortcut target or Command Prompt, a minimal command is:

```bat
"C:\Rallx\rallx-cheat-launcher.exe" --launch="rdr2-trainer.exe"
```

Use the trainer's filename only; Rallx resolves it inside your configured trainer folder. Omitted hotkeys fall back to the trainer's saved shortcut, then the global default shortcut. Omitted cheats use the trainer's saved cheats.

To override the hotkey and cheat sequence for one run:

```bat
"C:\Rallx\rallx-cheat-launcher.exe" --launch="rdr2-trainer.exe" --hotkey="insert" --defaultcheat="ctrl+num1,num3,ctrl+num5"
```

| Option | Purpose |
| --- | --- |
| `--launch="trainer.exe"` | Selects the trainer for single trainer mode. |
| `--hotkey="insert"` | Overrides its launch hotkey for this run. |
| `--defaultcheat="ctrl+num1,num3"` | Overrides its cheat sequence; commas separate combinations and `+` joins keys pressed together. `num1` means Numpad 1. |
| `--override` | Disables saved hotkey and cheat fallbacks, including the global shortcut fallback. Supply the hotkey and cheats you want in the command. |
| `--closeafterlaunch` | Exits Rallx after the launch sequence, leaving the trainer running. |

The trainer's auto-trigger setting still controls when cheats are sent. Without close-after-launch, a configured watched game exiting closes the launched trainer and also exits Rallx in single trainer mode. The tray menu offers **Launch [trainer]** and **Exit**.

Normal mode hidden in the tray remains normal mode: it chooses a trainer from the running watched game. Single trainer mode is started with `--launch` and targets one configured trainer.

## Gamepad controls

These use Xbox-style button names. On Home:

| Button | Action |
| --- | --- |
| D-pad Up / Down | Select a trainer |
| A | Launch the selected trainer |
| X | Edit the selected trainer |
| Y | Search |
| Select / View | Delete the selected trainer, with confirmation |
| RB | Copy the selected trainer's launch script |
| Start / Menu | Open Settings |
| B | Back or cancel |

In forms and Settings, use the D-pad to navigate, **A** to activate a control, and **B** to go back. Text fields support an on-screen keyboard. While it is open, **X** deletes a character, **Y** inserts a space, **LT** holds Shift, and **RT** closes the keyboard.

## Settings, backups, and updates

Rallx stores `config.json` **beside the app executable**. It contains your trainer folder selection, trainer settings, and preferences. Keep that folder writable.

To back up your setup, close Rallx and copy `config.json` along with your trainer folder. To update, exit Rallx, download the new build from [Releases](https://github.com/rajamuthukumaran-d/rallx-cheat-launcher/releases), and replace the app files while preserving `config.json`. If you move the trainer folder, select its new location in Settings.

## Troubleshooting

- **No trainers appear:** Check the selected trainer folder, ensure it contains the trainer `.exe` files, and press Refresh.
- **The global shortcut does nothing:** Keep Rallx running, configure the trainer's watched executable, and check that it matches the actual game process rather than its launcher. Try a different shortcut if another app already uses it.
- **The trainer opens but cheats do not activate:** With auto-trigger off, trigger the running trainer again. Check the recorded keys and include any required activation key. With auto-trigger on, try a longer delay. If the trainer runs elevated, Rallx must also run elevated for injected keys to reach it; use **Restart as admin**.
- **Drag and drop does not work:** Windows can block dragging from a non-elevated Explorer window into an elevated app. Use **Add → Trainer executable → Browse…** instead.
- **Settings do not persist:** Check that the folder containing Rallx is writable and that you are running the same copy of the app; each copy reads the `config.json` beside it.
- **The trainer stays open after Rallx exits:** Close-after-launch intentionally leaves it running. Disable that option and configure a watched executable if you want Rallx to close it when the game exits.

For bug reports, [open an issue](https://github.com/rajamuthukumaran-d/rallx-cheat-launcher/issues) with your Windows version, Rallx version, and steps to reproduce the problem.
