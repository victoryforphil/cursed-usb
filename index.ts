#!/usr/bin/env bun
import { createCliRenderer, TextRenderable, BoxRenderable, type KeyEvent } from "@opentui/core";
import { $ } from "bun";

interface USBDevice {
  bus: string;
  id: string;
  name: string;
  isDFU: boolean;
}

async function getUSBDevices(): Promise<USBDevice[]> {
  try {
    const result = await $`lsusb`.text();
    return result
      .split("\n")
      .filter((line) => line.trim())
      .map((line) => {
        const match = line.match(/Bus (\d+) Device \d+: ID ([0-9a-f:]+)\s*(.*)/i);
        if (!match) return null;
        const name = match[3] || "Unknown";
        return {
          bus: match[1],
          id: match[2],
          name: name.trim(),
          isDFU: /dfu|download|boot/i.test(name),
        };
      })
      .filter((d): d is USBDevice => d !== null);
  } catch {
    return [];
  }
}

async function main() {
  const renderer = await createCliRenderer({ targetFps: 5, exitOnCtrlC: true });
  renderer.setBackgroundColor("#0f172a");

  // Header
  const header = new BoxRenderable(renderer, {
    id: "header",
    width: "100%",
    height: 3,
    backgroundColor: "#1e3a5f",
    borderStyle: "single",
    borderColor: "#3b82f6",
    border: true,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
  });

  const headerText = new TextRenderable(renderer, {
    id: "header-text",
    content: "USB Devices",
    fg: "#38bdf8",
  });

  header.add(headerText);

  // Main content area
  const contentArea = new BoxRenderable(renderer, {
    id: "content-area",
    width: "100%",
    flexGrow: 1,
    flexDirection: "column",
    backgroundColor: "#1e293b",
    borderStyle: "single",
    borderColor: "#475569",
    border: true,
    padding: 1,
  });

  // Table header row
  const tableHeader = new TextRenderable(renderer, {
    id: "table-header",
    content: "",
    fg: "#94a3b8",
  });

  // Table content
  const tableContent = new TextRenderable(renderer, {
    id: "table-content",
    content: "",
    fg: "#e2e8f0",
  });

  contentArea.add(tableHeader);
  contentArea.add(tableContent);

  // Footer
  const footer = new BoxRenderable(renderer, {
    id: "footer",
    width: "100%",
    height: 3,
    backgroundColor: "#1e3a5f",
    borderStyle: "single",
    borderColor: "#475569",
    border: true,
    flexDirection: "row",
    alignItems: "center",
    paddingLeft: 2,
  });

  const footerText = new TextRenderable(renderer, {
    id: "footer-text",
    content: "q quit | r refresh",
    fg: "#64748b",
  });

  footer.add(footerText);

  // Build tree
  renderer.root.add(header);
  renderer.root.add(contentArea);
  renderer.root.add(footer);

  // State
  let devices: USBDevice[] = [];

  async function refresh() {
    devices = await getUSBDevices();
    render();
  }

  function render() {
    const w = renderer.terminalWidth;

    // Header
    const dfuCount = devices.filter((d) => d.isDFU).length;
    const dfuBadge = dfuCount > 0 ? `  \x1b[45;97;1m ${dfuCount} DFU \x1b[0m` : "";
    headerText.content = `\x1b[1;38;5;39mUSB Devices\x1b[0m  \x1b[2m(${devices.length})\x1b[0m${dfuBadge}`;

    // Table header
    const busW = 5;
    const idW = 11;
    const nameW = Math.max(30, w - busW - idW - 12);
    tableHeader.content = `\x1b[1m${"BUS".padEnd(busW)} ${"ID".padEnd(idW)} ${"NAME".padEnd(nameW)}\x1b[0m\n\x1b[2m${"─".repeat(busW)} ${"─".repeat(idW)} ${"─".repeat(nameW)}\x1b[0m`;

    // Table rows
    if (devices.length === 0) {
      tableContent.content = "\x1b[2mNo USB devices found\x1b[0m";
    } else {
      const rows = devices.map((d) => {
        const bus = d.bus.padEnd(busW);
        const id = d.id.padEnd(idW);
        const rawName = d.name;
        const name = rawName.length > nameW ? rawName.slice(0, nameW - 1) + "…" : rawName.padEnd(nameW);

        if (d.isDFU) {
          return `\x1b[1;93m${bus} ${id} ${name}\x1b[0m`;
        } else {
          return `\x1b[2m${bus}\x1b[0m \x1b[36m${id}\x1b[0m ${name}`;
        }
      });
      tableContent.content = rows.join("\n");
    }

    renderer.requestRender();
  }

  // Input
  renderer.keyInput.on("keypress", (key: KeyEvent) => {
    if (key.name === "q") process.exit(0);
    if (key.name === "r") refresh();
  });

  // Initial load
  await refresh();

  // Auto-refresh at 5Hz
  setInterval(refresh, 200);
}

main().catch((err) => {
  console.error("Error:", err);
  process.exit(1);
});
