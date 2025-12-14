#!/usr/bin/env bun
import { createCliRenderer, BoxRenderable, TextRenderable } from "@opentui/core";
import { exec } from "child_process";
import { promisify } from "util";
import * as fs from "fs";
import * as path from "path";
import * as os from "os";

const execAsync = promisify(exec);

// Configuration management
const CONFIG_DIR = path.join(os.homedir(), ".config", "cursed_tui");
const CONFIG_FILE = path.join(CONFIG_DIR, "config.json");

interface Config {
  filterDFU: boolean;
}

function ensureConfigDir() {
  if (!fs.existsSync(CONFIG_DIR)) {
    fs.mkdirSync(CONFIG_DIR, { recursive: true });
  }
}

function loadConfig(): Config {
  ensureConfigDir();
  try {
    if (fs.existsSync(CONFIG_FILE)) {
      const data = fs.readFileSync(CONFIG_FILE, "utf-8");
      return JSON.parse(data);
    }
  } catch (error) {
    // Silently handle error
  }
  return { filterDFU: false };
}

function saveConfig(config: Config) {
  ensureConfigDir();
  try {
    fs.writeFileSync(CONFIG_FILE, JSON.stringify(config, null, 2));
  } catch (error) {
    // Silently handle error
  }
}

// USB device interface
interface USBDevice {
  bus: string;
  device: string;
  id: string;
  description: string;
}

async function getUSBDevices(): Promise<USBDevice[]> {
  try {
    const { stdout } = await execAsync("lsusb");
    const lines = stdout.trim().split("\n");
    return lines.map((line) => {
      // Parse format: Bus 001 Device 002: ID 8087:8000 Intel Corp.
      const match = line.match(/Bus (\d+) Device (\d+): ID ([0-9a-f:]+)\s*(.+)?/i);
      if (match) {
        return {
          bus: match[1],
          device: match[2],
          id: match[3],
          description: match[4] || "Unknown",
        };
      }
      return {
        bus: "???",
        device: "???",
        id: "????:????",
        description: line,
      };
    });
  } catch (error) {
    // If lsusb is not available, return mock data for testing
    return [
      {
        bus: "001",
        device: "001",
        id: "1d6b:0002",
        description: "Linux Foundation 2.0 root hub",
      },
      {
        bus: "001",
        device: "002",
        id: "0483:df11",
        description: "STMicroelectronics STM Device in DFU Mode",
      },
      {
        bus: "002",
        device: "001",
        id: "1d6b:0003",
        description: "Linux Foundation 3.0 root hub",
      },
    ];
  }
}

function filterDevices(devices: USBDevice[], config: Config): USBDevice[] {
  if (config.filterDFU) {
    return devices.filter((dev) => 
      dev.description.toUpperCase().includes("DFU")
    );
  }
  return devices;
}

function formatDeviceList(devices: USBDevice[]): string {
  const lines = [
    `${"Bus".padEnd(6)} ${"Device".padEnd(8)} ${"ID".padEnd(12)} Description`,
  ];
  
  if (devices.length === 0) {
    lines.push("No devices found");
  } else {
    devices.forEach((dev) => {
      lines.push(`${dev.bus.padEnd(6)} ${dev.device.padEnd(8)} ${dev.id.padEnd(12)} ${dev.description}`);
    });
  }
  
  return lines.join("\n");
}

// Main application
let config = loadConfig();
let devices: USBDevice[] = [];

async function updateDevices() {
  devices = await getUSBDevices();
}

async function main() {
  // Create the renderer
  const renderer = await createCliRenderer({
    targetFps: 10,
    exitOnCtrlC: true,
  });

  // Create main container
  const container = new BoxRenderable(renderer, {
    id: "container",
    width: "100%",
    height: "100%",
    padding: 1,
    flexDirection: "column",
  });

  // Title
  const title = new TextRenderable(renderer, {
    id: "title",
    content: "Cursed USB Monitor",
    fg: "cyan",
    bold: true,
  });

  // Help text
  const help = new TextRenderable(renderer, {
    id: "help",
    content: "Press 'q' to quit | 'd' to toggle DFU filter | Refreshes every 1s",
    fg: "gray",
  });

  // Filter status
  const filterStatus = new TextRenderable(renderer, {
    id: "filter-status",
    content: "",
    fg: "yellow",
  });

  // Device list
  const deviceList = new TextRenderable(renderer, {
    id: "device-list",
    content: "",
    marginTop: 1,
  });

  // Status
  const status = new TextRenderable(renderer, {
    id: "status",
    content: "",
    fg: "gray",
    marginTop: 1,
  });

  // Build the tree
  renderer.root.add(container);
  container.add(title);
  container.add(help);
  container.add(filterStatus);
  container.add(deviceList);
  container.add(status);

  // Update UI with current devices
  function updateUI() {
    const filteredDevices = filterDevices(devices, config);
    
    // Update all text content
    filterStatus.content = `Filter: ${config.filterDFU ? "DFU Only ✓" : "All Devices"}`;
    deviceList.content = formatDeviceList(filteredDevices);
    status.content = `Total: ${filteredDevices.length} / ${devices.length} devices`;
    
    renderer.requestRender();
  }

  // Initial device fetch
  await updateDevices();
  updateUI();

  // Setup periodic updates (1000ms = 1s for more stability)
  setInterval(async () => {
    try {
      await updateDevices();
      updateUI();
    } catch (error) {
      // Silently handle update errors
    }
  }, 1000);

  // Handle keyboard input
  renderer.addInputHandler((sequence: string) => {
    if (sequence === "d" || sequence === "D") {
      config.filterDFU = !config.filterDFU;
      saveConfig(config);
      updateUI();
      return true;
    }
    return false;
  });
}

main().catch((error) => {
  console.error("Error:", error);
  process.exit(1);
});