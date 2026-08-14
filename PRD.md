# Rallx Cheat Launcher

I want to create an app to launch trainer for game. I call the app Rallx Cheat Launcher. It will be used in ROG Ally and steam deck so it needs to be optimised for handheld gaming pc with touch screen and gamepad, it will have following screens,

## Home:
 - It will have list of cheats in a list view
 - The list row will have
        - Trainer logo from exe
        - Name
        - Version
        - Size
        - Play icon
- When edit is enable following will also show up in the list
        - Copy Icon (to copy launch script)
        - Edit icon (Open Add screen and fill the existing data and change the title to edit)
        - Delete icon (Delete the trainer. Show confirmation before performing the action)
- A add (open add trainer page) and gear icon (open settings page) at the top
- Gamepad navigation
        - A - launch
        - X - Edit
        - Y - search
        - Select - Delete (Show confirmation popup)
        - Right bumper - copy launch script if configured
        - Start - open settings
        - checkbox to close the app after launching the trainer

## Settings:
- It will contain option to select trainer folder, default shortcut to launch trainer assosiated with the game that is running and theme
- Theme will have accent, background and style
- Option to close the app after launching trainer
- Option to run as administrator, plus a button to restart into it right away
    - Windows blocks injected key presses aimed at an elevated program, so default cheats only reach a trainer that needs admin rights when the app is elevated too
    - A process cannot elevate itself, so the toggle applies at the next startup and the button relaunches the app through a UAC prompt; the button is disabled when the app is already elevated

## Add/Edit trainer popup:
- This is a popup when a trainer is dragged into the app or pressed add icon from the home
- Show the selected trainer information and allow user to modify Name
- An option to choose game exe
- An optional executable to watch. After that app has been seen running and
  then closes, Rallx closes the trainer it launched and exits too. This works
  in both windowed and background/tray launches.
- Assign shortcut to launch trainer
- Add list of cheats need to enable (Will be a list containing key or key combination eg, Numpad 1, ctrl + numberpad 3, etc)
- When adding cheats to enable allow user to enter key or key combination by clicking on a record button

## Functionality,
- List all the trainer in a selected folder
- Clicking on the list or the play button or pressing A from gamepad will launch the trainer
- Optionally the app also have a feature to create launch option that make the app run in background and on press of a button, it will open the selected app
    - Launch option will look like following,
        - ```rallx-cheater.exe --launch="rdr2-trainer.exe" --hotkey="insert" --defaultcheat="ctrl+num1,num3,ctrl+num5"```
        - When the app launched with above launch option, the app UI doesn't show up instead it will open as a system tray icon (It will open in background)
        - When user hit insert, the app will launch the trainer rdr2-trainer.exe and programically press keys ctrl+num1, num3 and ctrl+num5
        - NOTE: rdr2-trainer.exe doesn't need the path, the app should launch the exe based on the selected path in the configuration as all the trainer reside inside single path
        - Only --launch is required. Anything left out falls back to that trainer's own saved settings - its launch shortcut, then the global default shortcut, and its saved default cheats - so ```rallx-cheater.exe --launch="rdr2-trainer.exe"``` behaves the same as launching it from the Home screen. The other flags are per-run overrides of those saved values
        - Passing --override turns the fallback off: the hotkey and cheats then come only from the command line, and a trainer with saved values contributes none of them. Use it to pin a script so editing the trainer later can't change what it does
    - When adding a trainer user can optinally configure hotkey to launch the trainer in the middle of the game
    - Can also configure default cheats from the trainer (By pressing keys and key combination programically)
- Dragging and dropping a exe will show add trainer window and confirming will move that file inside the trainer folder
- When adding a trainer, user has option to set launch option and default trainer shotcuts. Selecting these option only save them. When copy a launch script will be generated based on these selection
- Globally in settings and when launching individual trainer there is an option to close the app after launching the trainer. If enabled it will close the app once the trainer is launched and default cheats are entered
- A trainer with a watched executable stays open regardless of the immediate
  close-after-launch setting, because Rallx must remain running to detect when
  the watched app closes and clean up the trainer.
- If a trainer is launched from the app where the trainer had close after launch checked. It should be ignored and the app should follow if it is checked globally inside settings page. The individual "close after launch" option is only for generating launch option. If the launch option have "--closeafterlaunch" then it should close after launching and activating default cheats

## Design,
- Use @mockups/mockup.html for mockup
- Instructions to fetch the particular screen is mentioned in their selection
- Download the mockups inside mockups folder and hide it in .gitignore

## Tech stack
- Rust with slint
- Store configs inside config.json file next to the app executable (the
  selected trainer folder is one of the values stored in it)
