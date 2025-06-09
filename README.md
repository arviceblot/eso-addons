# ESO Addon Manager

A cross-platform, unofficial addon manager for The Elder Scrolls Online compatible with the Steam Deck.

![Image of main window](/docs/images/main.png)

## Features

- GUI ~~and CLI options~~ (CLI disabled due to breaking changes to support GUI)
- Install, remove, and search addons from [esoui.com](https://www.esoui.com)
- Cross-platform support for Linux, macOS, and Windows
- Specific support for ESO on the Steam Deck through Flathub
- Options to auto update Tamriel Trade Centre prices and HarvestMap data
- Light and dark UI themes
- No Java!
- Import managed addons from Minion
- Suggest installing addons for any missing depenencies

## Installing

### Linux

The recommended install is to use the version from flathub. This has the added benefit of centralized updates.

<a href="https://flathub.org/apps/details/com.arviceblot.eso-addon-manager"><img src="https://flathub.org/assets/badges/flathub-badge-en.png" alt="Flathub" height="50"/></a>

Or using the flatpak CLI:

```shell
flatpak install com.arviceblot.eso-addon-manager
```

#### AppImage

Download the `.AppImage` from the [releases](https://github.com/arviceblot/eso-addons/releases) page.


#### Build latest from source

```shell
cargo install --git https://github.com/arviceblot/eso-addons.git
```

### macOS and Windows

Downlaod the appropriate file for your OS from the [releases](https://github.com/arviceblot/eso-addons/releases) page.

## Running

### Flatpak

Either run using the desktop file flatpak installs or from the CLI:

```shell
flatpak run com.arviceblot.eso-addon-manager
```

### AppImage with AppImageLauncher

If the app was installed through AppImageLauncher, it should have automatically created a desktop entry. This can be added to steam as a non-steam game in desktop mode where it can then be launched in game mode. The app can be closed in game mode using the steam app menu.

### cargo

```shell
cargo run eso-addon-manager
```

## Updating

### AppImage

AppImageLauncher has the option to automatically install updates, this app has been designed to take advantage of that using GitHub release information.

## Legacy

This project was originally based on the work by Trojan295 at [Trojan295/eso-addons](https://github.com/Trojan295/eso-addons). It has since devolved into the abyss, but without his work I probably would not have even started on this silliness.
