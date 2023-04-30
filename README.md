# `clickd`, the *Internet Explorer Navigation Click*, for Linux

This is a ~~shitpost~~ serious computer program that plays the "Windows Start Navigation" *click* sound, on every mouse button press.

It uses `evdev` to listen to mouse button presses and so only works on Linux.

## Features

- *click*
- *clickclickclick*
- Configurable sound and volume
- Configurable set of buttons to trigger the sound on
- Tray Icon to disable the clicking (mostly just because it's funny to put the Internet Explorer logo in the Linux systray)

## Installation

Use `cargo build` to build from source. I'm not putting this on crates.io or a package repo, it's a joke program.

## Running

`clickd` takes an optional argument specifying the path to a configuration file.
See [`config.example.toml`](./config.example.toml) for an example.
If the path is omitted, the default configuration values documented in the example configuration are used.

There's also a systemd service file (for the user instance) at [`clickd.service`](./clickd.service).
