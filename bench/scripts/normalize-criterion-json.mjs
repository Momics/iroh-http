import fs from "node:fs";
import path from "node:path";

const [, , criterionRoot, throughputOut, latencyOut] = process.argv;
if (!criterionRoot || !throughputOut || !latencyOut) {
  console.error("usage: node bench/scripts/normalize-criterion-json.mjs <criterion_root> <throughput_out> <latency_out>");
  process.exit(1);
}

function walk(dir, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(p, out);
    if (entry.isFile() && entry.name === "estimates.json" && p.includes(`${path.sep}new${path.sep}`)) out.push(p);
  }
  return out;
}

const files = fs.existsSync(criterionRoot) ? walk(criterionRoot) : [];
const throughput = [];
const latency = [];

for (const file of files) {
  const parsed = JSON.parse(fs.readFileSync(file, "utf8"));
  const ns = parsed?.mean?.point_estimate;
  if (typeof ns !== "number") continue;

  const relative = file.replace(`${criterionRoot}${path.sep}`, "");
  const benchName = relative.split(path.sep).slice(0, -2).join("/").replace(/\\/g, "/");

  if (benchName.startsWith("throughput/")) {
    const sizeMatch = benchName.match(/\/(\d+)$/);
    const size = sizeMatch ? Number(sizeMatch[1]) : 1024;
    const seconds = ns / 1_000_000_000;
    const mbPerSec = (size / (1024 * 1024)) / seconds;
    throughput.push({ name: `rust/${benchName}`, unit: "MB/s", value: mbPerSec });
  } else {
    latency.push({ name: `rust/${benchName}`, unit: "us", value: ns / 1_000 });
  }
}

fs.writeFileSync(throughputOut, JSON.stringify(throughput, null, 2));
fs.writeFileSync(latencyOut, JSON.stringify(latency, null, 2));
