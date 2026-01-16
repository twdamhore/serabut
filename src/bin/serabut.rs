use anyhow::Result;
use clap::{Parser, Subcommand};
use serabut::{
    find_boot_by_mac, find_entry_by_label, find_entry_by_mac, list_profiles, normalize_mac,
    profile_exists, profiles_dir, read_boot_entries, read_mac_entries, resolve_target,
    validate_label, validate_mac, write_boot_entries, write_mac_entries, BootEntry, SerabutError,
};

#[derive(Parser)]
#[command(name = "serabut")]
#[command(about = "Lightweight bare metal PXE provisioning tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage discovered MAC addresses
    Mac {
        #[command(subcommand)]
        action: MacCommands,
    },
    /// Manage boot assignments
    Boot {
        #[command(subcommand)]
        action: BootCommands,
    },
    /// Manage boot profiles
    Profiles {
        #[command(subcommand)]
        action: ProfileCommands,
    },
}

#[derive(Subcommand)]
enum MacCommands {
    /// List all discovered MAC addresses (sorted by last seen)
    List,
    /// Assign or clear a label for a MAC address
    Label {
        /// MAC address (format: aa:bb:cc:dd:ee:ff)
        mac: String,
        /// Label to assign (a-z only, max 8 chars, or "" to clear)
        label: String,
    },
    /// Remove a MAC address from the list
    Remove {
        /// MAC address (format: aa:bb:cc:dd:ee:ff)
        mac: String,
    },
}

#[derive(Subcommand)]
enum BootCommands {
    /// Add a boot assignment
    Add {
        /// MAC address or label
        target: String,
        /// Profile name
        profile: String,
    },
    /// Remove a boot assignment
    Remove {
        /// MAC address or label
        target: String,
    },
    /// List active boot assignments
    List,
}

#[derive(Subcommand)]
enum ProfileCommands {
    /// List available boot profiles
    List,
}

fn cmd_mac_list() -> Result<()> {
    let mut entries = read_mac_entries()?;

    if entries.is_empty() {
        println!("No MAC addresses discovered yet.");
        return Ok(());
    }

    // Sort by last_seen descending (most recent first)
    entries.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));

    // Print header
    println!("{:<10} {:<19} {:<24}", "LABEL", "MAC", "LAST SEEN");
    println!("{}", "-".repeat(55));

    for entry in entries {
        let label_display = if entry.label.is_empty() {
            "-"
        } else {
            &entry.label
        };
        println!(
            "{:<10} {:<19} {:<24}",
            label_display,
            entry.mac,
            entry.last_seen.format("%Y-%m-%d %H:%M:%S UTC")
        );
    }

    Ok(())
}

fn cmd_mac_label(mac: &str, label: &str) -> Result<()> {
    validate_mac(mac)?;
    validate_label(label)?;

    let mut entries = read_mac_entries()?;
    let mac = normalize_mac(mac);

    // Check if MAC exists
    let mac_idx = find_entry_by_mac(&entries, &mac)
        .ok_or_else(|| SerabutError::MacNotFound(mac.clone()))?;

    // If setting a non-empty label, check uniqueness
    if !label.is_empty() {
        if let Some(idx) = find_entry_by_label(&entries, label) {
            if idx != mac_idx {
                return Err(SerabutError::LabelTaken {
                    label: label.to_string(),
                    mac: entries[idx].mac.clone(),
                }
                .into());
            }
        }
    }

    let old_label = entries[mac_idx].label.clone();
    entries[mac_idx].label = label.to_string();
    write_mac_entries(&entries)?;

    if label.is_empty() {
        if old_label.is_empty() {
            println!("Label already cleared for {}", mac);
        } else {
            println!("Cleared label '{}' from {}", old_label, mac);
        }
    } else {
        println!("Assigned label '{}' to {}", label, mac);
    }

    Ok(())
}

fn cmd_mac_remove(mac: &str) -> Result<()> {
    validate_mac(mac)?;

    let mut entries = read_mac_entries()?;
    let mac = normalize_mac(mac);

    let idx = find_entry_by_mac(&entries, &mac)
        .ok_or_else(|| SerabutError::MacNotFound(mac.clone()))?;

    let removed = entries.remove(idx);
    write_mac_entries(&entries)?;

    let label_info = if removed.label.is_empty() {
        String::new()
    } else {
        format!(" ({})", removed.label)
    };
    println!("Removed {}{}", mac, label_info);

    Ok(())
}

fn cmd_boot_add(target: &str, profile: &str) -> Result<()> {
    // Resolve target to MAC address
    let mac = resolve_target(target)?;

    // Validate profile exists
    if !profile_exists(profile) {
        return Err(anyhow::anyhow!(
            "Profile '{}' not found in {}",
            profile,
            profiles_dir().display()
        ));
    }

    // Read existing boot entries
    let mut entries = read_boot_entries()?;

    // Check if already assigned
    if let Some(idx) = find_boot_by_mac(&entries, &mac) {
        let old_profile = entries[idx].profile.clone();
        entries[idx] = BootEntry::new(mac.clone(), profile.to_string());
        write_boot_entries(&entries)?;
        println!(
            "Updated {} from '{}' to '{}'",
            mac, old_profile, profile
        );
    } else {
        entries.push(BootEntry::new(mac.clone(), profile.to_string()));
        write_boot_entries(&entries)?;
        println!("Assigned '{}' to {}", profile, mac);
    }

    Ok(())
}

fn cmd_boot_remove(target: &str) -> Result<()> {
    // Resolve target to MAC address
    let mac = resolve_target(target)?;

    // Read existing boot entries
    let mut entries = read_boot_entries()?;

    // Find and remove
    if let Some(idx) = find_boot_by_mac(&entries, &mac) {
        let removed = entries.remove(idx);
        write_boot_entries(&entries)?;
        println!("Removed boot assignment '{}' from {}", removed.profile, mac);
    } else {
        println!("No boot assignment found for {}", mac);
    }

    Ok(())
}

fn cmd_boot_list() -> Result<()> {
    let entries = read_boot_entries()?;

    if entries.is_empty() {
        println!("No active boot assignments.");
        return Ok(());
    }

    // Get MAC entries to show labels
    let mac_entries = read_mac_entries().unwrap_or_default();

    // Print header
    println!("{:<10} {:<19} {:<16} {:<24}", "LABEL", "MAC", "PROFILE", "ASSIGNED AT");
    println!("{}", "-".repeat(72));

    for entry in entries {
        // Find label for this MAC
        let label = mac_entries
            .iter()
            .find(|m| m.mac == entry.mac)
            .map(|m| m.label.as_str())
            .unwrap_or("");

        let label_display = if label.is_empty() { "-" } else { label };

        println!(
            "{:<10} {:<19} {:<16} {:<24}",
            label_display,
            entry.mac,
            entry.profile,
            entry.assigned_at.format("%Y-%m-%d %H:%M:%S UTC")
        );
    }

    Ok(())
}

fn cmd_profiles_list() -> Result<()> {
    let profiles = list_profiles()?;

    if profiles.is_empty() {
        println!("No profiles found in {}/", profiles_dir().display());
        return Ok(());
    }

    println!("Available profiles:");
    for profile in profiles {
        println!("  {}", profile);
    }

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Mac { action } => match action {
            MacCommands::List => cmd_mac_list(),
            MacCommands::Label { mac, label } => cmd_mac_label(&mac, &label),
            MacCommands::Remove { mac } => cmd_mac_remove(&mac),
        },
        Commands::Boot { action } => match action {
            BootCommands::Add { target, profile } => cmd_boot_add(&target, &profile),
            BootCommands::Remove { target } => cmd_boot_remove(&target),
            BootCommands::List => cmd_boot_list(),
        },
        Commands::Profiles { action } => match action {
            ProfileCommands::List => cmd_profiles_list(),
        },
    }
}
