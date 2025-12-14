# cursed-usb
Cursed LSUSB TUI in Bun 

A lightweight terminal user interface (TUI) app for monitoring USB devices in real-time. Built with [Bun](https://bun.sh/) and [OpenTUI](https://github.com/sst/opentui), this is a modern replacement for `watch -n 0.1 lsusb`.

## Features

- 🔄 Real-time USB device monitoring (updates every 100ms)
- 🎨 Clean, easy-to-read terminal interface
- 🔍 Filter devices by type (DFU mode support)
- 💾 Persistent configuration saved to `~/.config/cursed_tui`
- ⚡ Fast and lightweight

## Prerequisites

- [Bun](https://bun.sh/) runtime installed
- `lsusb` command available (usually from `usbutils` package)

## Installation

### Install as a global command

```bash
# Clone the repository
git clone https://github.com/victoryforphil/cursed-usb.git
cd cursed-usb

# Install dependencies
npm install

# Install globally using npm
npm install -g .
```

After installation, you can run the command from anywhere:

```bash
cursed_usb
```

### Alternative: Install using Bun

```bash
# Clone the repository
git clone https://github.com/victoryforphil/cursed-usb.git
cd cursed-usb

# Install dependencies
bun install

# Link globally
bun link

# In another directory, link the package
bun link cursed-usb
```

### Manual Installation

If you prefer to run it without global installation:

```bash
# Clone and install dependencies
git clone https://github.com/victoryforphil/cursed-usb.git
cd cursed-usb
npm install

# Run directly
bun run index.ts
# or
npm start
```

## Usage

Once installed globally, simply run:

```bash
cursed_usb
```

### Keyboard Controls

- `q` or `Ctrl+C` - Quit the application
- `d` - Toggle DFU device filter (shows only devices with "DFU" in the name)

### Configuration

The application saves your filter preferences to `~/.config/cursed_tui/config.json`.

Example configuration:
```json
{
  "filterDFU": false
}
```

## Development

### Running in Development Mode

```bash
bun run index.ts
```

### Building

This is a TypeScript application that runs directly with Bun, no build step required.

## Requirements

- **Bun**: v1.0.0 or higher
- **lsusb**: Available on most Linux distributions via the `usbutils` package
  - Ubuntu/Debian: `sudo apt-get install usbutils`
  - Fedora/RHEL: `sudo dnf install usbutils`
  - Arch: `sudo pacman -S usbutils`

## License

GPL-3.0 - See LICENSE file for details
