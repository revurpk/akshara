//! `fatlib`: merges per-architecture Mach-O static libraries (`.a`) into one
//! universal ("fat") library, like `lipo -create`, on any host. This lets
//! Linux and Windows build the macOS and iOS-simulator core without Xcode.
//!
//! ```text
//! cargo run -p fatlib -- -o libakshara.a arm64/libakshara.a x86_64/libakshara.a
//! ```
//!
//! The architecture of each input is read from its first Mach-O member, so
//! the inputs can be given in any order. Two inputs of the same architecture
//! are refused.

use std::process::ExitCode;

const FAT_MAGIC: u32 = 0xcafe_babe;
const MH_MAGIC_64: u32 = 0xfeed_facf;
/// Alignment of each slice (2^14 = 16 KiB, the arm64 page size).
const ALIGN_LOG2: u32 = 14;

/// Reads the CPU type and subtype of the first 64-bit Mach-O object in a
/// BSD or GNU `ar` archive.
fn archive_arch(data: &[u8]) -> Result<(u32, u32), String> {
    if !data.starts_with(b"!<arch>\n") {
        return Err("not an ar archive".into());
    }
    let mut pos = 8;
    while pos + 60 <= data.len() {
        let header = &data[pos..pos + 60];
        if &header[58..60] != b"`\n" {
            return Err(format!("damaged archive member header at {pos}"));
        }
        let field = |a: usize, b: usize| std::str::from_utf8(&header[a..b]).unwrap_or("").trim().to_owned();
        let name = field(0, 16);
        let size: usize = field(48, 58).parse().map_err(|_| format!("bad member size at {pos}"))?;
        let start = pos + 60;
        let end = start.checked_add(size).filter(|e| *e <= data.len()).ok_or("member runs past the end")?;
        // BSD long names: "#1/<len>", the name precedes the data.
        let skip = name.strip_prefix("#1/").and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
        let body = &data[(start + skip).min(end)..end];
        if body.len() >= 12 && u32::from_le_bytes([body[0], body[1], body[2], body[3]]) == MH_MAGIC_64 {
            let cpu = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            let sub = u32::from_le_bytes([body[8], body[9], body[10], body[11]]) & 0x00ff_ffff;
            return Ok((cpu, sub));
        }
        pos = end + (end & 1); // members are 2-byte aligned
    }
    Err("no 64-bit Mach-O object found (is this a macOS or iOS library?)".into())
}

fn arch_name(cpu: u32) -> &'static str {
    match cpu {
        0x0100_000c => "arm64",
        0x0100_0007 => "x86_64",
        _ => "unknown",
    }
}

/// Builds the universal file from (cpu, subtype, data) slices.
fn fat(slices: &[(u32, u32, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let align = 1usize << ALIGN_LOG2;
    let mut out = Vec::new();
    out.extend_from_slice(&FAT_MAGIC.to_be_bytes());
    out.extend_from_slice(&(slices.len() as u32).to_be_bytes());
    let mut offset = (8 + 20 * slices.len()).next_multiple_of(align);
    let mut layout = Vec::new();
    for (cpu, sub, data) in slices {
        let off32 = u32::try_from(offset).map_err(|_| "library too large for a fat file")?;
        let len32 = u32::try_from(data.len()).map_err(|_| "library too large for a fat file")?;
        for v in [*cpu, *sub, off32, len32, ALIGN_LOG2] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        layout.push(offset);
        offset = (offset + data.len()).next_multiple_of(align);
    }
    for ((_, _, data), at) in slices.iter().zip(layout) {
        out.resize(at, 0);
        out.extend_from_slice(data);
    }
    Ok(out)
}

fn run(args: &[String]) -> Result<(), String> {
    let (output, inputs) = match args {
        [flag, out, inputs @ ..] if flag == "-o" && !inputs.is_empty() => (out, inputs),
        _ => return Err("usage: fatlib -o <universal.a> <arch1.a> <arch2.a>...".into()),
    };
    let mut slices: Vec<(u32, u32, Vec<u8>)> = Vec::new();
    for path in inputs {
        let data = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let (cpu, sub) = archive_arch(&data).map_err(|e| format!("{path}: {e}"))?;
        if slices.iter().any(|s| s.0 == cpu) {
            return Err(format!("{path}: a second {} library", arch_name(cpu)));
        }
        eprintln!("{path}: {}", arch_name(cpu));
        slices.push((cpu, sub, data));
    }
    // lipo's order: x86_64 before arm64.
    slices.sort_by_key(|s| std::cmp::Reverse(s.0 & 0xff == 7));
    let bytes = fat(&slices)?;
    std::fs::write(output, bytes).map_err(|e| format!("{output}: {e}"))?;
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fatlib: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An archive with a symbol table and one tiny Mach-O "object".
    fn archive(cpu: u32) -> Vec<u8> {
        let mut obj = Vec::new();
        obj.extend_from_slice(&MH_MAGIC_64.to_le_bytes());
        obj.extend_from_slice(&cpu.to_le_bytes());
        obj.extend_from_slice(&0u32.to_le_bytes());
        let member = |name: &str, body: &[u8]| {
            let mut m = format!("{name:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", 0, 0, 0, 644, body.len()).into_bytes();
            m.extend_from_slice(body);
            if body.len() % 2 == 1 {
                m.push(b'\n');
            }
            m
        };
        let mut a = b"!<arch>\n".to_vec();
        a.extend(member("__.SYMDEF", b"xyz"));
        let mut long = b"lib.o\0\0\0".to_vec();
        long.extend_from_slice(&obj);
        a.extend(member("#1/8", &long));
        a
    }

    #[test]
    fn detects_architectures_and_builds_a_fat_file() {
        assert_eq!(archive_arch(&archive(0x0100_000c)), Ok((0x0100_000c, 0)));
        assert!(archive_arch(b"junk").is_err());
        let slices = vec![(0x0100_0007, 3, archive(0x0100_0007)), (0x0100_000c, 0, archive(0x0100_000c))];
        let f = fat(&slices).expect("fat");
        assert_eq!(&f[0..4], &FAT_MAGIC.to_be_bytes());
        assert_eq!(u32::from_be_bytes([f[4], f[5], f[6], f[7]]), 2);
        let offset = u32::from_be_bytes([f[16], f[17], f[18], f[19]]) as usize;
        assert_eq!(offset % (1 << ALIGN_LOG2), 0);
        assert!(f[offset..].starts_with(b"!<arch>\n"));
    }
}
