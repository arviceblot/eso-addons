# ESO Addon Manager

A cross-platform, unofficial addon manager for The Elder Scrolls Online designed for the Steam Deck.

![Image of main window](/docs/images/main.png)

## Features

- GUI and CLI options
- Install, remove, and search addons from [esoui.com](https://www.esoui.com)
- Cross-platform support for Linux, macOS, and Windows
- Specific support for ESO on the Steam Deck through Flathub
- Option to update Tamriel Trade Centre prices
- No Java!
- Import managed addons from Minion

### Planned Features

- Show additional addon details in search
- Browse all addons by category and other filters
- Import already installed addons to manage (without Minion backup)

## Installing

TODO: Publish to Flathub and crates.io

### AppImage

TODO:

## Building from source

```shell
cargo install --git https://github.com/arviceblot/eso-addons.git
```

## Running

Run the app.

### AppImage with AppImageLauncher

TODO:

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
