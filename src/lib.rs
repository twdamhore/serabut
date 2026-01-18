use rusqlite::{Connection, Result};
use std::path::Path;

pub const DB_PATH: &str = "/var/lib/serabut/serabut.db";
pub const DATA_DIR: &str = "/var/lib/serabut";

pub fn open_db() -> Result<Connection> {
    Connection::open(DB_PATH)
}

pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS hardware (
            mac TEXT PRIMARY KEY
        )",
        [],
    )?;
    Ok(())
}

pub fn arm(conn: &Connection, mac: &str) -> Result<bool> {
    let rows = conn.execute(
        "INSERT OR IGNORE INTO hardware (mac) VALUES (?1)",
        [mac],
    )?;
    Ok(rows > 0)
}

pub fn disarm(conn: &Connection, mac: &str, force: bool) -> Result<bool> {
    let rows = conn.execute(
        "DELETE FROM hardware WHERE mac = ?1",
        [mac],
    )?;
    if rows == 0 && !force {
        return Ok(false);
    }
    Ok(true)
}

pub fn is_armed(conn: &Connection, mac: &str) -> Result<bool> {
    let mut stmt = conn.prepare("SELECT 1 FROM hardware WHERE mac = ?1")?;
    let exists = stmt.exists([mac])?;
    Ok(exists)
}

pub fn list(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT mac FROM hardware ORDER BY mac")?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    let mut macs = Vec::new();
    for mac in rows {
        macs.push(mac?);
    }
    Ok(macs)
}

pub fn boot_ipxe_path(mac: &str) -> std::path::PathBuf {
    Path::new(DATA_DIR).join(mac).join("boot.ipxe")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    #[test]
    fn test_init_db_creates_table() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        // Verify table exists by querying it
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM hardware", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_init_db_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        init_db(&conn).unwrap(); // Should not fail
    }

    #[test]
    fn test_arm_new_mac_returns_true() {
        let conn = setup_test_db();
        let result = arm(&conn, "aa-bb-cc-dd-ee-ff").unwrap();
        assert!(result, "arming new MAC should return true");
    }

    #[test]
    fn test_arm_existing_mac_returns_false() {
        let conn = setup_test_db();
        arm(&conn, "aa-bb-cc-dd-ee-ff").unwrap();
        let result = arm(&conn, "aa-bb-cc-dd-ee-ff").unwrap();
        assert!(!result, "arming existing MAC should return false");
    }

    #[test]
    fn test_arm_multiple_different_macs() {
        let conn = setup_test_db();
        assert!(arm(&conn, "11-22-33-44-55-66").unwrap());
        assert!(arm(&conn, "aa-bb-cc-dd-ee-ff").unwrap());
        assert!(arm(&conn, "00-00-00-00-00-00").unwrap());

        // Verify all three are in the database
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM hardware", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_disarm_existing_mac_returns_true() {
        let conn = setup_test_db();
        arm(&conn, "aa-bb-cc-dd-ee-ff").unwrap();
        let result = disarm(&conn, "aa-bb-cc-dd-ee-ff", false).unwrap();
        assert!(result, "disarming existing MAC should return true");
    }

    #[test]
    fn test_disarm_nonexistent_mac_without_force_returns_false() {
        let conn = setup_test_db();
        let result = disarm(&conn, "aa-bb-cc-dd-ee-ff", false).unwrap();
        assert!(!result, "disarming non-existent MAC without force should return false");
    }

    #[test]
    fn test_disarm_nonexistent_mac_with_force_returns_true() {
        let conn = setup_test_db();
        let result = disarm(&conn, "aa-bb-cc-dd-ee-ff", true).unwrap();
        assert!(result, "disarming non-existent MAC with force should return true");
    }

    #[test]
    fn test_disarm_actually_removes_mac() {
        let conn = setup_test_db();
        arm(&conn, "aa-bb-cc-dd-ee-ff").unwrap();
        assert!(is_armed(&conn, "aa-bb-cc-dd-ee-ff").unwrap());

        disarm(&conn, "aa-bb-cc-dd-ee-ff", false).unwrap();
        assert!(!is_armed(&conn, "aa-bb-cc-dd-ee-ff").unwrap());
    }

    #[test]
    fn test_is_armed_returns_true_for_armed_mac() {
        let conn = setup_test_db();
        arm(&conn, "aa-bb-cc-dd-ee-ff").unwrap();
        assert!(is_armed(&conn, "aa-bb-cc-dd-ee-ff").unwrap());
    }

    #[test]
    fn test_is_armed_returns_false_for_unarmed_mac() {
        let conn = setup_test_db();
        assert!(!is_armed(&conn, "aa-bb-cc-dd-ee-ff").unwrap());
    }

    #[test]
    fn test_is_armed_returns_false_after_disarm() {
        let conn = setup_test_db();
        arm(&conn, "aa-bb-cc-dd-ee-ff").unwrap();
        disarm(&conn, "aa-bb-cc-dd-ee-ff", false).unwrap();
        assert!(!is_armed(&conn, "aa-bb-cc-dd-ee-ff").unwrap());
    }

    #[test]
    fn test_boot_ipxe_path_construction() {
        let path = boot_ipxe_path("aa-bb-cc-dd-ee-ff");
        assert_eq!(
            path,
            std::path::PathBuf::from("/var/lib/serabut/aa-bb-cc-dd-ee-ff/boot.ipxe")
        );
    }

    #[test]
    fn test_boot_ipxe_path_with_different_macs() {
        let path1 = boot_ipxe_path("00-00-00-00-00-00");
        let path2 = boot_ipxe_path("ff-ff-ff-ff-ff-ff");

        assert_eq!(
            path1,
            std::path::PathBuf::from("/var/lib/serabut/00-00-00-00-00-00/boot.ipxe")
        );
        assert_eq!(
            path2,
            std::path::PathBuf::from("/var/lib/serabut/ff-ff-ff-ff-ff-ff/boot.ipxe")
        );
    }

    #[test]
    fn test_arm_disarm_arm_cycle() {
        let conn = setup_test_db();

        // Arm
        assert!(arm(&conn, "aa-bb-cc-dd-ee-ff").unwrap());
        assert!(is_armed(&conn, "aa-bb-cc-dd-ee-ff").unwrap());

        // Disarm
        assert!(disarm(&conn, "aa-bb-cc-dd-ee-ff", false).unwrap());
        assert!(!is_armed(&conn, "aa-bb-cc-dd-ee-ff").unwrap());

        // Re-arm
        assert!(arm(&conn, "aa-bb-cc-dd-ee-ff").unwrap());
        assert!(is_armed(&conn, "aa-bb-cc-dd-ee-ff").unwrap());
    }

    #[test]
    fn test_empty_mac_string() {
        let conn = setup_test_db();
        assert!(arm(&conn, "").unwrap());
        assert!(is_armed(&conn, "").unwrap());
        assert!(disarm(&conn, "", false).unwrap());
        assert!(!is_armed(&conn, "").unwrap());
    }

    #[test]
    fn test_mac_case_sensitivity() {
        let conn = setup_test_db();
        arm(&conn, "AA-BB-CC-DD-EE-FF").unwrap();

        // SQLite TEXT comparison is case-sensitive by default
        assert!(is_armed(&conn, "AA-BB-CC-DD-EE-FF").unwrap());
        assert!(!is_armed(&conn, "aa-bb-cc-dd-ee-ff").unwrap());
    }

    #[test]
    fn test_list_empty_database() {
        let conn = setup_test_db();
        let macs = list(&conn).unwrap();
        assert!(macs.is_empty());
    }

    #[test]
    fn test_list_single_mac() {
        let conn = setup_test_db();
        arm(&conn, "aa-bb-cc-dd-ee-ff").unwrap();

        let macs = list(&conn).unwrap();
        assert_eq!(macs, vec!["aa-bb-cc-dd-ee-ff"]);
    }

    #[test]
    fn test_list_multiple_macs_sorted() {
        let conn = setup_test_db();
        arm(&conn, "cc-cc-cc-cc-cc-cc").unwrap();
        arm(&conn, "aa-aa-aa-aa-aa-aa").unwrap();
        arm(&conn, "bb-bb-bb-bb-bb-bb").unwrap();

        let macs = list(&conn).unwrap();
        assert_eq!(
            macs,
            vec![
                "aa-aa-aa-aa-aa-aa",
                "bb-bb-bb-bb-bb-bb",
                "cc-cc-cc-cc-cc-cc"
            ]
        );
    }

    #[test]
    fn test_list_after_disarm() {
        let conn = setup_test_db();
        arm(&conn, "aa-bb-cc-dd-ee-ff").unwrap();
        arm(&conn, "11-22-33-44-55-66").unwrap();

        disarm(&conn, "aa-bb-cc-dd-ee-ff", false).unwrap();

        let macs = list(&conn).unwrap();
        assert_eq!(macs, vec!["11-22-33-44-55-66"]);
    }
}
