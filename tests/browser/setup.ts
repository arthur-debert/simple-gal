import { execSync } from "child_process";
import path from "path";

const ROOT = path.resolve(__dirname, "../..");
const GENERATED_DIR = path.join(__dirname, "generated");
const TEMP_DIR = path.join(ROOT, ".simple-gal-browser-temp");
const FIXTURE_SOURCE = path.join(ROOT, "fixtures", "browser-content");

export default function globalSetup() {
  console.log("Generating browser test fixtures...");
  execSync(
    `cargo run -- build --source "${FIXTURE_SOURCE}" --output "${GENERATED_DIR}" --temp-dir "${TEMP_DIR}"`,
    { cwd: ROOT, stdio: "inherit" },
  );
  console.log("Browser test fixtures ready.");
}
