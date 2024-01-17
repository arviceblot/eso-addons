# ESO Addon Manager

A cross-platform, unofficial addon manager for The Elder Scrolls Online compatible with the Steam Deck.

![Image of main window](/docs/images/main.png)

## Features

- GUI ~~and CLI options~~ (CLI disabled due to breaking changes to support GUI)
- Install, remove, and search addons from [esoui.com](https://www.esoui.com)
- Cross-platform support for Linux, macOS, and Windows
- Specific support for ESO on the Steam Deck through AppImage
- Options to auto update Tamriel Trade Centre prices and HarvestMap data
- No Java!
- Import managed addons from Minion
- Suggest installing addons for any missing depenencies

## Installing

TODO: Publish to Flathub and crates.io

### AppImage

TODO:

## Building from source

```shell
cargo install --git https://github.com/arviceblot/eso-addons.git
```

## Running

### AppImage with AppImageLauncher

If the app was installed through AppImageLauncher, it should have automatically created a desktop entry. This can be added to steam as a non-steam game in desktop mode where it can then be launched in game mode. The app can be closed in game mode using the steam app menu.

### CLI

```shell
❯ eso-addons --help
eso-addons 0.1.0

CLI tool for managing addons for The Elder Scrolls Online

USAGE:
    eso-addons [OPTIONS] <SUBCOMMAND>

OPTIONS:
    -c, --config <CONFIG>    Path to TOML config file
    -h, --help               Print help information
    -V, --version            Print version information

SUBCOMMANDS:
    add       Add a new addon
    help      Print this message or the help of the given subcommand(s)
    remove    Uninstall addon
    search    Search addons
    show      Show addon details
    update    Update addons
```

## Updating

TODO: cargo

### AppImage

AppImageLauncher has the option to automatically install updates, this app has been designed to take advantage of that using GitHub release information.

## Legacy

This project was originally based on the work by Trojan295 at [Trojan295/eso-addons](https://github.com/Trojan295/eso-addons). It has since devolved into the abyss, but without his work I probably would not have even started on this silliness.
