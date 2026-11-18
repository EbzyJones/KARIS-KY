use std::fs;
use std::path::Path;

/// A minimal address representation for the REPL CLI.
/// In a full integration this would be the escrow crate's `Address` type.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Address(String);

/// Parses a single CSV row of the form `address,amount`.
/// Trims whitespace, rejects malformed rows with a clear error.
fn parse_csv_row(line: &str) -> Result<(Address, i128), String> {
    let mut parts = line.splitn(2, ',');
    let address = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("missing address in row: {line:?}"))?;
    let amount_str = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("missing amount in row: {line:?}"))?;
    let amount: i128 = amount_str
        .parse()
        .map_err(|_| format!("invalid amount {amount_str:?} in row: {line:?}"))?;
    Ok((Address(address.to_string()), amount))
}

/// Parses CSV content into a list of `(Address, amount)` pairs.
/// Skips empty lines and lines starting with `#`.
fn parse_fund_batch_csv(content: &str) -> Result<Vec<(Address, i128)>, String> {
    let mut entries = Vec::new();
    for (idx, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let entry = parse_csv_row(line)
            .map_err(|e| format!("line {}: {e}", idx + 1))?;
        entries.push(entry);
    }
    Ok(entries)
}

/// Reads and parses the CSV file at `path`.
fn load_fund_batch_csv(path: &str) -> Result<Vec<(Address, i128)>, String> {
    let content = fs::read_to_string(Path::new(path))
        .map_err(|e| format!("failed to read CSV file {path:?}: {e}"))?;
    parse_fund_batch_csv(&content)
}

/// Mirrors `escrow/src/funding.rs::fund_batch_impl` semantics:
/// invokes the escrow `fund_batch` entrypoint with the parsed batch.
fn fund_batch(entries: &[(Address, i128)]) -> Result<(), String> {
    if entries.is_empty() {
        return Err("fund_batch requires at least one entry".to_string());
    }
    for (address, amount) in entries {
        if *amount <= 0 {
            return Err(format!(
                "fund_batch amount must be positive for {}: {amount}",
                address.0
            ));
        }
    }
    // In a full integration this would call the escrow contract entrypoint.
    Ok(())
}

fn parse_input(input: &str) -> Result<&str, &'static str> {
    let trimmed = input.trim();

    if trimmed.is_empty() {
        return Err("command cannot be empty");
    }

    Ok(trimmed)
}

fn run_fund_batch(csv_path: &str) -> Result<(), String> {
    let entries = load_fund_batch_csv(csv_path)?;
    fund_batch(&entries)?;
    println!("fund_batch invoked with {} entries", entries.len());
    for (address, amount) in &entries {
        println!("  {} -> {}", address.0, amount);
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let command = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "status".to_string());

    match parse_input(&command) {
        Ok(command) => {
            println!("KARIS-KY REPL CLI");
            if command == "fund-batch" {
                let csv_path = match args.get(2) {
                    Some(path) => path,
                    None => {
                        eprintln!("usage: fund-batch <csv-path>");
                        std::process::exit(1);
                    }
                };
                if let Err(message) = run_fund_batch(csv_path) {
                    eprintln!("{message}");
                    std::process::exit(1);
                }
            } else {
                println!("Command received: {command}");
            }
        }
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fund_batch, parse_fund_batch_csv, parse_input, Address};

    #[test]
    fn parse_input_accepts_non_empty_commands() {
        assert_eq!(parse_input("  status  ").unwrap(), "status");
    }

    #[test]
    fn parse_input_rejects_empty_input() {
        assert!(parse_input("   ").is_err());
    }

    #[test]
    fn parse_fund_batch_csv_parses_entries_and_calls_fund_batch() {
        let csv = "\
# address,amount
GABC123, 100
  GDEF456 ,250
";
        let entries = parse_fund_batch_csv(csv).unwrap();
        assert_eq!(
            entries,
            vec![
                (Address("GABC123".to_string()), 100),
                (Address("GDEF456".to_string()), 250),
            ]
        );
        // The constructed batch call receives the parsed entries.
        assert!(fund_batch(&entries).is_ok());
    }

    #[test]
    fn parse_fund_batch_csv_rejects_malformed_rows() {
        assert!(parse_fund_batch_csv("GABC123").is_err());
        assert!(parse_fund_batch_csv("GABC123,notanumber").is_err());
        assert!(parse_fund_batch_csv(",100").is_err());
    }

    #[test]
    fn fund_batch_rejects_empty_and_non_positive() {
        assert!(fund_batch(&[]).is_err());
        assert!(fund_batch(&[(Address("GABC123".to_string()), 0)]).is_err());
    }
}