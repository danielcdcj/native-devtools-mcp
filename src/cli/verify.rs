use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

use super::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

const REPO: &str = "sh3ll3x3c/native-devtools-mcp";
const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run() {
    println!();
    println!("{BOLD}native-devtools-mcp v{VERSION} — Binary Verification{RESET}");
    println!();

    // Step 1: Hash the current binary
    let exe_path = match std::env::current_exe() {
        Ok(p) => fs::canonicalize(&p).unwrap_or(p),
        Err(e) => {
            println!("{RED}✗{RESET} Failed to determine binary path: {e}");
            std::process::exit(1);
        }
    };

    let local_hash = match hash_file(&exe_path) {
        Ok(h) => h,
        Err(e) => {
            println!("{RED}✗{RESET} Failed to hash binary: {e}");
            std::process::exit(1);
        }
    };

    println!("  Binary:  {}", exe_path.display());
    println!("  SHA-256: {DIM}{local_hash}{RESET}");
    println!();

    // Step 2: Fetch expected checksums from GitHub
    let checksums_url =
        format!("https://github.com/{REPO}/releases/download/v{VERSION}/checksums.txt");

    println!("  Fetching checksums from GitHub release v{VERSION}...");

    let checksums_text = match fetch_checksums(&checksums_url) {
        Ok(text) => text,
        Err(e) => {
            println!();
            println!("  {YELLOW}?{RESET} Could not fetch checksums: {e}");
            println!();
            println!("  This may mean:");
            println!("  - No internet connection");
            println!("  - This is a development build with no matching release");
            println!("  - The release does not include checksums yet");
            println!();
            println!("  Your local hash: {BOLD}{local_hash}{RESET}");
            println!("  Compare manually at: https://github.com/{REPO}/releases/tag/v{VERSION}");
            println!();
            std::process::exit(2);
        }
    };

    let expected_hash = match find_expected_hash(&checksums_text) {
        Some(hash) => hash,
        None => {
            println!();
            println!(
                "  {YELLOW}?{RESET} No matching checksum found for this platform in the release."
            );
            println!("  Your local hash: {local_hash}");
            println!("  Check manually at: https://github.com/{REPO}/releases/tag/v{VERSION}");
            println!();
            std::process::exit(2);
        }
    };

    if local_hash == expected_hash {
        println!();
        println!("  {GREEN}✓ Verified{RESET} — binary matches the official GitHub release.");
        println!();
    } else {
        println!();
        println!("  {RED}✗ Mismatch{RESET} — binary does NOT match the official release!");
        println!();
        println!("  Local:    {local_hash}");
        println!("  Expected: {expected_hash}");
        println!();
        std::process::exit(1);
    }
}

fn hash_file(path: &Path) -> Result<String, String> {
    let data = fs::read(path).map_err(|e| format!("read error: {e}"))?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Ok(format!("{:x}", hasher.finalize()))
}

fn fetch_checksums(url: &str) -> Result<String, String> {
    let response = ureq::get(url).call().map_err(|e| format!("{e}"))?;
    response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("failed to read response: {e}"))
}

fn find_expected_hash(checksums: &str) -> Option<String> {
    let platform_binary = if cfg!(target_os = "macos") {
        "native-devtools-mcp (aarch64-apple-darwin)"
    } else if cfg!(target_os = "windows") {
        "native-devtools-mcp.exe (x86_64-pc-windows-msvc)"
    } else {
        return None;
    };

    for line in checksums.lines() {
        // Format: "hash  filename"
        if let Some((hash, name)) = line.split_once("  ") {
            if name.trim() == platform_binary {
                return Some(hash.trim().to_string());
            }
        }
    }
    None
}
