#!/usr/bin/env node

const os = require("os");

const override = (process.env.VITE_NETWORK_HOST || "").trim();
if (override.length > 0) {
  console.log(override);
  process.exit(0);
}

const ifacePreference = (process.env.VITE_NETWORK_INTERFACE || "")
  .split(",")
  .map((value) => value.trim())
  .filter(Boolean);

const interfaces = os.networkInterfaces();
const candidates = [];

for (const [name, entries] of Object.entries(interfaces)) {
  if (!entries) continue;
  for (const entry of entries) {
    if (entry.family !== "IPv4" || entry.internal) continue;
    candidates.push({ name, address: entry.address });
  }
}

let selected;

for (const preferred of ifacePreference) {
  selected = candidates.find((candidate) => candidate.name === preferred);
  if (selected) break;
}

if (!selected) {
  selected = candidates[0];
}

if (selected) {
  console.log(selected.address);
  process.exit(0);
}

const hostname = os.hostname();
if (hostname) {
  console.log(hostname.toLowerCase());
  process.exit(0);
}

console.log("localhost");
