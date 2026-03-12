/// Standalone test binary to diagnose Shentong (OSCAR) quoting / search_path
/// behaviour when SELECTing from a table.
///
/// Run with:
///   cargo run --bin shentong_query_test
///
/// Requires aci.dll (Shentong client library) on PATH or in the working dir.

use shentong::Connection;

fn main() {
    let host = "192.168.3.34:2003";
    let user = "SYSDBA";
    let pass = "szoscar55";

    println!("=== Shentong Query Pattern Test ===");
    println!("Connecting to {} as {} ...", host, user);

    let conn = match Connection::connect(user, pass, host) {
        Ok(c) => {
            println!("OK: Connected successfully.\n");
            c
        }
        Err(e) => {
            eprintln!("FATAL: Could not connect: {}", e);
            return;
        }
    };

    // Verify the connection works at all
    match conn.query("SELECT 1 FROM DUAL", &[]) {
        Ok(_) => println!("Sanity check: SELECT 1 FROM DUAL => OK\n"),
        Err(e) => {
            eprintln!("Sanity check FAILED: {}\nAborting.", e);
            return;
        }
    }

    // ---- Early diagnostics (before test patterns, to avoid ACCESS_VIOLATION) ----
    println!("=== Early Diagnostics ===\n");

    // 1. Find what case the table is stored in
    {
        let sql = "SELECT table_name, owner FROM all_tables WHERE table_name LIKE '%TEST%'";
        print!("  [1] {} => ", sql);
        match conn.query(sql, &[]) {
            Ok(rows) => {
                let mut found = false;
                for row_result in rows {
                    match row_result {
                        Ok(row) => {
                            let tname: String = row.get::<_, String>(0).unwrap_or_default();
                            let owner: String = row.get::<_, String>(1).unwrap_or_default();
                            print!("{}.{}  ", owner, tname);
                            found = true;
                        }
                        Err(e) => print!("[row error: {}] ", e),
                    }
                }
                if !found { print!("(no rows)"); }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }
    }

    // 2. Explicit upper match
    {
        let sql = "SELECT table_name, owner FROM all_tables WHERE UPPER(table_name) = 'TEST_01'";
        print!("  [2] {} => ", sql);
        match conn.query(sql, &[]) {
            Ok(rows) => {
                let mut found = false;
                for row_result in rows {
                    match row_result {
                        Ok(row) => {
                            let tname: String = row.get::<_, String>(0).unwrap_or_default();
                            let owner: String = row.get::<_, String>(1).unwrap_or_default();
                            print!("{}.{}  ", owner, tname);
                            found = true;
                        }
                        Err(e) => print!("[row error: {}] ", e),
                    }
                }
                if !found { print!("(no rows)"); }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }
    }

    // 3. Explicit lower match
    {
        let sql = "SELECT table_name, owner FROM all_tables WHERE LOWER(table_name) = 'test_01'";
        print!("  [3] {} => ", sql);
        match conn.query(sql, &[]) {
            Ok(rows) => {
                let mut found = false;
                for row_result in rows {
                    match row_result {
                        Ok(row) => {
                            let tname: String = row.get::<_, String>(0).unwrap_or_default();
                            let owner: String = row.get::<_, String>(1).unwrap_or_default();
                            print!("{}.{}  ", owner, tname);
                            found = true;
                        }
                        Err(e) => print!("[row error: {}] ", e),
                    }
                }
                if !found { print!("(no rows)"); }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }
    }

    // 4. All SYSDBA tables
    {
        let sql = "SELECT table_name, owner FROM all_tables WHERE owner = 'SYSDBA' ORDER BY table_name";
        print!("  [4] {} => ", sql);
        match conn.query(sql, &[]) {
            Ok(rows) => {
                let mut found = false;
                for row_result in rows {
                    match row_result {
                        Ok(row) => {
                            let tname: String = row.get::<_, String>(0).unwrap_or_default();
                            let owner: String = row.get::<_, String>(1).unwrap_or_default();
                            print!("{}.{}  ", owner, tname);
                            found = true;
                        }
                        Err(e) => print!("[row error: {}] ", e),
                    }
                }
                if !found { print!("(no rows)"); }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }
    }

    // 5. Current user
    {
        let sql = "SELECT CURRENT_USER FROM DUAL";
        print!("  [5] {} => ", sql);
        match conn.query(sql, &[]) {
            Ok(rows) => {
                let mut found = false;
                for row_result in rows {
                    match row_result {
                        Ok(row) => {
                            let val: String = row.get::<_, String>(0).unwrap_or_default();
                            print!("{} ", val);
                            found = true;
                        }
                        Err(e) => print!("[row error: {}] ", e),
                    }
                }
                if !found { print!("(no rows)"); }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }
    }

    // 6. Try SELECT * FROM test_01 (lowercase, no quotes) after SET search_path
    {
        let set_sql = "SET search_path TO SYSDBA";
        print!("  [6a] {} => ", set_sql);
        match conn.execute(set_sql, &[]) {
            Ok(_) => println!("OK"),
            Err(e) => println!("ERROR: {}", e),
        }

        let sql = "SELECT * FROM test_01";
        print!("  [6b] {} => ", sql);
        match conn.query(sql, &[]) {
            Ok(rows) => {
                let mut count = 0usize;
                for row_result in rows {
                    match row_result {
                        Ok(_) => count += 1,
                        Err(e) => { print!("[row error: {}] ", e); break; }
                    }
                }
                println!("OK ({} rows)", count);
            }
            Err(e) => println!("ERROR: {}", e),
        }
    }

    // 7. SHOW search_path
    {
        let sql = "SHOW search_path";
        print!("  [7] {} => ", sql);
        match conn.query(sql, &[]) {
            Ok(rows) => {
                let mut found = false;
                for row_result in rows {
                    match row_result {
                        Ok(row) => {
                            let val: String = row.get::<_, String>(0).unwrap_or_default();
                            print!("{} ", val);
                            found = true;
                        }
                        Err(e) => print!("[row error: {}] ", e),
                    }
                }
                if !found { print!("(no rows)"); }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }
    }

    // 8. All lowercase schema.table without quotes (PostgreSQL normalization test)
    {
        let sql = "SELECT * FROM sysdba.test_01";
        print!("  [8] {} => ", sql);
        match conn.query(sql, &[]) {
            Ok(rows) => {
                let mut count = 0usize;
                for row_result in rows {
                    match row_result {
                        Ok(_) => count += 1,
                        Err(e) => { print!("[row error: {}] ", e); break; }
                    }
                }
                println!("OK ({} rows)", count);
            }
            Err(e) => println!("ERROR: {}", e),
        }
    }

    // 9. Lowercase with double quotes (forces exact case)
    {
        let sql = r#"SELECT * FROM "sysdba"."test_01""#;
        print!("  [9] {} => ", sql);
        match conn.query(sql, &[]) {
            Ok(rows) => {
                let mut count = 0usize;
                for row_result in rows {
                    match row_result {
                        Ok(_) => count += 1,
                        Err(e) => { print!("[row error: {}] ", e); break; }
                    }
                }
                println!("OK ({} rows)", count);
            }
            Err(e) => println!("ERROR: {}", e),
        }
    }

    // 10. pg_class lookup FIRST (before anything that might crash)
    {
        println!("\n  --- pg_class lookup ---");
        let sql = "SELECT c.relname, n.nspname \
                   FROM pg_class c \
                   JOIN pg_namespace n ON c.relnamespace = n.oid \
                   WHERE UPPER(c.relname) LIKE '%TEST%'";
        print!("  [pg] {} => ", sql);
        match conn.query(sql, &[]) {
            Ok(rows) => {
                let mut found = false;
                for row_result in rows {
                    match row_result {
                        Ok(row) => {
                            let relname: String = row.get::<_, String>(0).unwrap_or_default();
                            let nspname: String = row.get::<_, String>(1).unwrap_or_default();
                            print!("nsp='{}' rel='{}' | ", nspname, relname);
                            found = true;
                        }
                        Err(e) => { print!("[row error: {}] ", e); break; }
                    }
                }
                if !found { print!("(no rows in pg_class)"); }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }

        // List all namespaces
        let sql2 = "SELECT nspname FROM pg_namespace ORDER BY nspname";
        print!("  [pg] namespaces => ");
        match conn.query(sql2, &[]) {
            Ok(rows) => {
                for row_result in rows {
                    match row_result {
                        Ok(row) => {
                            let n: String = row.get::<_, String>(0).unwrap_or_default();
                            print!("{} ", n);
                        }
                        Err(e) => { print!("[err: {}] ", e); break; }
                    }
                }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }

        // current database
        print!("  [pg] current_database() => ");
        match conn.query("SELECT current_database()", &[]) {
            Ok(rows) => {
                for r in rows {
                    match r {
                        Ok(row) => { let v: String = row.get::<_, String>(0).unwrap_or_default(); print!("{}", v); }
                        Err(e) => print!("[err: {}]", e),
                    }
                }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }
    }

    // 11. More access patterns
    {
        println!("\n  --- More access patterns ---");
        for (label, sql) in &[
            ("3-part name", "SELECT * FROM OSRDB.SYSDBA.TEST_01"),
            ("3-part quoted", r#"SELECT * FROM "OSRDB"."SYSDBA"."TEST_01""#),
            ("CREATE + SELECT", ""),  // special handling below
            ("user_tables view", "SELECT table_name FROM user_tables WHERE ROWNUM <= 5"),
            ("tab view", "SELECT tname FROM TAB WHERE ROWNUM <= 5"),
            ("dba_tables", "SELECT owner, table_name FROM dba_tables WHERE table_name = 'TEST_01'"),
        ] {
            if *label == "CREATE + SELECT" {
                print!("  [try] CREATE TABLE _ACI_TEST_ => ");
                // Create a table, query it, drop it
                let fresh = match Connection::connect("SYSDBA", "szoscar55", "192.168.3.34:2003") {
                    Ok(c) => c,
                    Err(e) => { println!("CONNECT ERROR: {}", e); continue; }
                };
                match fresh.execute("CREATE TABLE _ACI_TEST_TBL_ (id NUMBER, val VARCHAR2(50))", &[]) {
                    Ok(_) => {
                        print!("created. INSERT => ");
                        match fresh.execute("INSERT INTO _ACI_TEST_TBL_ VALUES (1, 'hello')", &[]) {
                            Ok(_) => {
                                print!("inserted. SELECT => ");
                                match fresh.query("SELECT * FROM _ACI_TEST_TBL_", &[]) {
                                    Ok(rows) => {
                                        let mut count = 0;
                                        for r in rows { if r.is_ok() { count += 1; } else { break; } }
                                        print!("OK ({} rows). ", count);
                                    }
                                    Err(e) => print!("SELECT FAILED: {}. ", e),
                                }
                            }
                            Err(e) => print!("INSERT FAILED: {}. ", e),
                        }
                        // Cleanup
                        match fresh.execute("DROP TABLE _ACI_TEST_TBL_", &[]) {
                            Ok(_) => println!("Dropped."),
                            Err(e) => println!("DROP FAILED: {}", e),
                        }
                    }
                    Err(e) => println!("CREATE FAILED: {}", e),
                }
                continue;
            }

            print!("  [try] {} : {} => ", label, sql);
            let fresh = match Connection::connect("SYSDBA", "szoscar55", "192.168.3.34:2003") {
                Ok(c) => c,
                Err(e) => { println!("CONNECT ERROR: {}", e); continue; }
            };
            match fresh.query(sql, &[]) {
                Ok(rows) => {
                    let mut count = 0;
                    let mut first_vals = Vec::new();
                    for r in rows {
                        match r {
                            Ok(row) => {
                                count += 1;
                                if count <= 3 {
                                    let v: String = row.get::<_, String>(0).unwrap_or_default();
                                    first_vals.push(v);
                                }
                            }
                            Err(e) => { print!("[row err: {}] ", e); break; }
                        }
                    }
                    println!("OK ({} rows) first: {:?}", count, first_vals);
                }
                Err(e) => println!("ERROR: {}", e),
            }
        }
    }

    // 12. Key test: list user_tables and try to SELECT from each
    {
        println!("\n  --- user_tables listing + SELECT test ---");
        let fresh = Connection::connect("SYSDBA", "szoscar55", "192.168.3.34:2003").unwrap();
        let sql = "SELECT table_name FROM user_tables WHERE table_name LIKE '%TEST%'";
        print!("  [ut] {} => ", sql);
        match fresh.query(sql, &[]) {
            Ok(rows) => {
                let mut names = Vec::new();
                for r in rows {
                    match r {
                        Ok(row) => {
                            let n: String = row.get::<_, String>(0).unwrap_or_default();
                            print!("'{}' ", n);
                            names.push(n);
                        }
                        Err(e) => { print!("[err: {}] ", e); break; }
                    }
                }
                println!();

                // Now try SELECT from each found table
                for name in &names {
                    let fresh2 = Connection::connect("SYSDBA", "szoscar55", "192.168.3.34:2003").unwrap();
                    // Try the exact name as returned from user_tables
                    let sql1 = format!("SELECT * FROM {}", name);
                    print!("    SELECT * FROM {} => ", name);
                    match fresh2.query(&sql1, &[]) {
                        Ok(rows) => {
                            let mut count = 0;
                            for r in rows { if r.is_ok() { count += 1; } else { break; } }
                            println!("OK ({} rows)", count);
                        }
                        Err(e) => println!("ERROR: {}", e),
                    }

                    // Try with double quotes around the exact name
                    let fresh3 = Connection::connect("SYSDBA", "szoscar55", "192.168.3.34:2003").unwrap();
                    let sql2 = format!(r#"SELECT * FROM "{}""#, name);
                    print!("    {} => ", sql2);
                    match fresh3.query(&sql2, &[]) {
                        Ok(rows) => {
                            let mut count = 0;
                            for r in rows { if r.is_ok() { count += 1; } else { break; } }
                            println!("OK ({} rows)", count);
                        }
                        Err(e) => println!("ERROR: {}", e),
                    }

                    // Try lowercase quoted
                    let fresh4 = Connection::connect("SYSDBA", "szoscar55", "192.168.3.34:2003").unwrap();
                    let lower_name = name.to_lowercase();
                    let sql3 = format!(r#"SELECT * FROM "{}""#, lower_name);
                    print!("    {} => ", sql3);
                    match fresh4.query(&sql3, &[]) {
                        Ok(rows) => {
                            let mut count = 0;
                            for r in rows { if r.is_ok() { count += 1; } else { break; } }
                            println!("OK ({} rows)", count);
                        }
                        Err(e) => println!("ERROR: {}", e),
                    }
                }
            }
            Err(e) => println!("ERROR: {}", e),
        }
    }

    // 13. Compare with a CREATE'd table behavior
    {
        println!("\n  --- CREATE vs existing table comparison ---");
        let fresh = Connection::connect("SYSDBA", "szoscar55", "192.168.3.34:2003").unwrap();

        // Create table, commit, then try from fresh connection
        print!("  [cmp] CREATE TABLE _CMP_TEST_ => ");
        match fresh.execute("CREATE TABLE _CMP_TEST_ (id NUMBER)", &[]) {
            Ok(_) => {
                println!("OK");
                // Check user_tables
                print!("  [cmp] user_tables name => ");
                match fresh.query("SELECT table_name FROM user_tables WHERE table_name LIKE '%CMP%'", &[]) {
                    Ok(rows) => {
                        for r in rows {
                            match r {
                                Ok(row) => { let n: String = row.get::<_, String>(0).unwrap_or_default(); print!("'{}' ", n); }
                                Err(e) => print!("[err: {}] ", e),
                            }
                        }
                        println!();
                    }
                    Err(e) => println!("ERROR: {}", e),
                }

                // Try from fresh connection
                let fresh2 = Connection::connect("SYSDBA", "szoscar55", "192.168.3.34:2003").unwrap();
                print!("  [cmp] SELECT from new conn => ");
                match fresh2.query("SELECT * FROM _CMP_TEST_", &[]) {
                    Ok(rows) => {
                        let mut count = 0;
                        for r in rows { if r.is_ok() { count += 1; } else { break; } }
                        println!("OK ({} rows)", count);
                    }
                    Err(e) => println!("ERROR: {}", e),
                }

                // Cleanup
                let _ = fresh.execute("DROP TABLE _CMP_TEST_", &[]);
            }
            Err(e) => println!("CREATE FAILED: {}", e),
        }
    }

    // 14. Check if TEST_01 is in a different database
    {
        println!("\n  --- Database check ---");
        let fresh = Connection::connect("SYSDBA", "szoscar55", "192.168.3.34:2003").unwrap();
        print!("  [db] current_database() => ");
        match fresh.query("SELECT current_database()", &[]) {
            Ok(rows) => {
                for r in rows {
                    match r {
                        Ok(row) => { let v: String = row.get::<_, String>(0).unwrap_or_default(); print!("{}", v); }
                        Err(_) => {}
                    }
                }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }
        // List databases
        print!("  [db] v$database => ");
        match fresh.query("SELECT name FROM v$database", &[]) {
            Ok(rows) => {
                for r in rows {
                    match r {
                        Ok(row) => { let v: String = row.get::<_, String>(0).unwrap_or_default(); print!("{} ", v); }
                        Err(_) => {}
                    }
                }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }
    }

    // 15. Try information_schema and SYS_DATABASE
    {
        println!("\n  --- information_schema and database listing ---");
        let fresh = Connection::connect("SYSDBA", "szoscar55", "192.168.3.34:2003").unwrap();

        // information_schema.tables
        print!("  [is] information_schema.tables TEST_01 => ");
        match fresh.query("SELECT table_schema, table_name, table_catalog FROM information_schema.tables WHERE table_name = 'TEST_01'", &[]) {
            Ok(rows) => {
                let mut found = false;
                for r in rows {
                    match r {
                        Ok(row) => {
                            let schema: String = row.get::<_, String>(0).unwrap_or_default();
                            let name: String = row.get::<_, String>(1).unwrap_or_default();
                            let catalog: String = row.get::<_, String>(2).unwrap_or_default();
                            print!("catalog='{}' schema='{}' name='{}' ", catalog, schema, name);
                            found = true;
                        }
                        Err(e) => { print!("[err: {}] ", e); break; }
                    }
                }
                if !found { print!("(no rows)"); }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }

        // SYS_DATABASE
        print!("  [sd] SYS_DATABASE => ");
        match fresh.query("SELECT * FROM SYS_DATABASE", &[]) {
            Ok(rows) => {
                for r in rows {
                    match r {
                        Ok(row) => {
                            // Try to get first few columns
                            let c0: String = row.get::<_, String>(0).unwrap_or_default();
                            let c1: String = row.get::<_, String>(1).unwrap_or("".to_string());
                            print!("'{}' '{}' | ", c0, c1);
                        }
                        Err(e) => { print!("[err: {}] ", e); break; }
                    }
                }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }

        // v$database with more columns
        print!("  [vd] v$database => ");
        match fresh.query("SELECT name FROM v$database", &[]) {
            Ok(rows) => {
                for r in rows {
                    match r {
                        Ok(row) => { let v: String = row.get::<_, String>(0).unwrap_or_default(); print!("{} ", v); }
                        Err(_) => {}
                    }
                }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }
    }

    // 16. Try connecting with database name in connect string (Oracle EZConnect style)
    {
        println!("\n  --- Connections with database name ---");
        for (label, cs) in &[
            ("host:port/OSRDB", "192.168.3.34:2003/OSRDB"),
            ("host:port/osrdb", "192.168.3.34:2003/osrdb"),
            ("host:port:OSRDB", "192.168.3.34:2003:OSRDB"),
            ("//host:port/OSRDB", "//192.168.3.34:2003/OSRDB"),
        ] {
            print!("  [conn] {} => ", label);
            match Connection::connect("SYSDBA", "szoscar55", cs) {
                Ok(c) => {
                    match c.query("SELECT * FROM TEST_01", &[]) {
                        Ok(rows) => {
                            let mut count = 0;
                            for r in rows { if r.is_ok() { count += 1; } else { break; } }
                            println!("CONNECTED + SELECT OK ({} rows)", count);
                        }
                        Err(e) => {
                            // Also check current_database
                            let db = match c.query("SELECT current_database()", &[]) {
                                Ok(r2) => {
                                    let mut d = String::new();
                                    for r in r2 { if let Ok(row) = r { d = row.get::<_, String>(0).unwrap_or_default(); } }
                                    d
                                }
                                Err(_) => "?".to_string(),
                            };
                            println!("CONNECTED (db={}) but SELECT failed: {}", db, e);
                        }
                    }
                }
                Err(e) => println!("Connect failed: {}", e),
            }
        }
    }

    // 17. Check all_tables for database/tablespace info
    {
        println!("\n  --- all_tables extended info ---");
        let fresh = Connection::connect("SYSDBA", "szoscar55", "192.168.3.34:2003").unwrap();
        print!("  [at] all_tables TEST_01 extended => ");
        match fresh.query("SELECT owner, table_name, tablespace_name FROM all_tables WHERE table_name = 'TEST_01'", &[]) {
            Ok(rows) => {
                for r in rows {
                    match r {
                        Ok(row) => {
                            let owner: String = row.get::<_, String>(0).unwrap_or_default();
                            let name: String = row.get::<_, String>(1).unwrap_or_default();
                            let tbs: String = row.get::<_, String>(2).unwrap_or("(null)".to_string());
                            print!("owner='{}' name='{}' tablespace='{}' ", owner, name, tbs);
                        }
                        Err(e) => { print!("[err: {}] ", e); break; }
                    }
                }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }

        // Also check _CMP_TEST_ (the one we can access)
        let _ = fresh.execute("CREATE TABLE _CMP_CHECK_ (id NUMBER)", &[]);
        print!("  [at] all_tables _CMP_CHECK_ extended => ");
        match fresh.query("SELECT owner, table_name, tablespace_name FROM all_tables WHERE table_name = '_CMP_CHECK_'", &[]) {
            Ok(rows) => {
                for r in rows {
                    match r {
                        Ok(row) => {
                            let owner: String = row.get::<_, String>(0).unwrap_or_default();
                            let name: String = row.get::<_, String>(1).unwrap_or_default();
                            let tbs: String = row.get::<_, String>(2).unwrap_or("(null)".to_string());
                            print!("owner='{}' name='{}' tablespace='{}' ", owner, name, tbs);
                        }
                        Err(e) => { print!("[err: {}] ", e); break; }
                    }
                }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }
        let _ = fresh.execute("DROP TABLE _CMP_CHECK_", &[]);
    }

    let tests: Vec<(&str, TestKind)> = vec![
        // (a) schema.table without quotes
        (
            "SELECT * FROM SYSDBA.TEST_01",
            TestKind::SingleQuery,
        ),
        // (b) both quoted
        (
            r#"SELECT * FROM "SYSDBA"."TEST_01""#,
            TestKind::SingleQuery,
        ),
        // (c) only table quoted
        (
            r#"SELECT * FROM SYSDBA."TEST_01""#,
            TestKind::SingleQuery,
        ),
        // (d) only schema quoted
        (
            r#"SELECT * FROM "SYSDBA".TEST_01"#,
            TestKind::SingleQuery,
        ),
        // (e) SET search_path + SELECT in one statement
        (
            "SET search_path TO SYSDBA; SELECT * FROM TEST_01",
            TestKind::SingleQuery,
        ),
        // (f) SET search_path first via execute, then SELECT
        (
            "SET search_path TO SYSDBA  ->  SELECT * FROM TEST_01",
            TestKind::TwoStep,
        ),
        // (g) lowercase table name
        (
            "SELECT * FROM test_01",
            TestKind::SingleQuery,
        ),
        // (h) all lowercase schema.table
        (
            "SELECT * FROM sysdba.test_01",
            TestKind::SingleQuery,
        ),
    ];

    let mut results: Vec<(&str, &str)> = Vec::new();

    for (i, (label, kind)) in tests.iter().enumerate() {
        let tag = (b'a' + i as u8) as char;
        print!("({}) {} ... ", tag, label);

        let outcome = match kind {
            TestKind::SingleQuery => try_query(&conn, label),
            TestKind::TwoStep => try_two_step(&conn),
        };

        match outcome {
            Ok(count) => {
                let msg = "OK";
                println!("{} ({} rows)", msg, count);
                results.push((label, msg));
            }
            Err(e) => {
                let msg = "FAIL";
                println!("{}: {}", msg, e);
                results.push((label, msg));
            }
        }
    }

    // Also try some extra diagnostic queries
    println!("\n=== Extra diagnostics ===");

    // What does current_schema / search_path look like?
    for diag_sql in &[
        "SELECT SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') FROM DUAL",
        "SHOW search_path",
        "SELECT CURRENT_SCHEMA FROM DUAL",
        "SELECT CURRENT_USER FROM DUAL",
    ] {
        print!("  {} => ", diag_sql);
        match conn.query(diag_sql, &[]) {
            Ok(rows) => {
                let mut found = false;
                for row_result in rows {
                    match row_result {
                        Ok(row) => {
                            let val: String = row.get::<_, String>(0).unwrap_or_default();
                            print!("{} ", val);
                            found = true;
                        }
                        Err(e) => print!("[row error: {}] ", e),
                    }
                }
                if !found {
                    print!("(no rows)");
                }
                println!();
            }
            Err(e) => println!("ERROR: {}", e),
        }
    }

    // Check if the table actually exists via catalog
    println!("\n=== Catalog check: does TEST_01 exist? ===");
    let catalog_sql = "SELECT owner, table_name FROM all_tables WHERE table_name = 'TEST_01'";
    print!("  {} => ", catalog_sql);
    match conn.query(catalog_sql, &[]) {
        Ok(rows) => {
            let mut found = false;
            for row_result in rows {
                match row_result {
                    Ok(row) => {
                        let owner: String = row.get::<_, String>(0).unwrap_or_default();
                        let tname: String = row.get::<_, String>(1).unwrap_or_default();
                        print!("{}.{}  ", owner, tname);
                        found = true;
                    }
                    Err(e) => print!("[row error: {}] ", e),
                }
            }
            if !found {
                print!("(no rows — table not found in catalog!)");
            }
            println!();
        }
        Err(e) => println!("ERROR: {}", e),
    }

    // Summary
    println!("\n=== Summary ===");
    for (i, (label, status)) in results.iter().enumerate() {
        let tag = (b'a' + i as u8) as char;
        println!("  ({}) {} => {}", tag, label, status);
    }
}

enum TestKind {
    SingleQuery,
    TwoStep,
}

fn try_query(conn: &Connection, sql: &str) -> Result<usize, String> {
    let rows = conn.query(sql, &[]).map_err(|e| format!("{}", e))?;
    let mut count = 0usize;
    for row_result in rows {
        row_result.map_err(|e| format!("row error: {}", e))?;
        count += 1;
    }
    Ok(count)
}

fn try_two_step(conn: &Connection) -> Result<usize, String> {
    // Step 1: SET search_path
    conn.execute("SET search_path TO SYSDBA", &[])
        .map_err(|e| format!("SET search_path failed: {}", e))?;

    // Step 2: plain SELECT
    let rows = conn
        .query("SELECT * FROM TEST_01", &[])
        .map_err(|e| format!("SELECT after SET search_path failed: {}", e))?;

    let mut count = 0usize;
    for row_result in rows {
        row_result.map_err(|e| format!("row error: {}", e))?;
        count += 1;
    }
    Ok(count)
}
