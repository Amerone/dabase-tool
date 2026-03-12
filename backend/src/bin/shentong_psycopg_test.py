"""
Shentong OSCAR database connection test via psycopg2 and raw PostgreSQL wire protocol.

FINDINGS:
  - Shentong OSCAR rejects PostgreSQL protocol v3.0 (used by psycopg2):
      "FATAL, Unsupported frontend protocol:196608"
  - Shentong accepts PostgreSQL protocol v2.0 (code=131072) but uses NON-STANDARD
    fixed-field sizes: database(128) + user(64) instead of PG's database(64) + user(32).
  - Authentication is MD5 (type 5), same as PostgreSQL.
  - SSL negotiation times out (SSLRequest not supported).
  - psycopg2 CANNOT connect because it only speaks protocol v3.0.
  - A raw socket implementation using v2.0 with db(128)+user(64) fields successfully
    reaches the MD5 auth challenge. Full authentication requires the correct password.

CONCLUSION:
  psycopg2 will NOT work with Shentong OSCAR. Use ODBC (via the OSCAR ODBC DRIVER)
  or implement a custom client using the modified PG v2.0 wire protocol.
"""

import sys
import socket
import struct
import hashlib

HOST = "192.168.3.34"
PORT = 2003
USER = "SYSDBA"
PASSWORD = "szoscar55"
DBNAME = "OSRDB"

QUERIES = [
    "SELECT * FROM TEST_01",
    'SELECT * FROM "TEST_01"',
    "SELECT * FROM test_01",
    'SELECT * FROM "test_01"',
    "SELECT * FROM SYSDBA.TEST_01",
    'SELECT * FROM "SYSDBA"."TEST_01"',
    "SELECT * FROM pg_class WHERE relname = 'TEST_01' OR relname = 'test_01'",
    "SHOW search_path",
]


# ---------------------------------------------------------------------------
# Wire protocol helpers
# ---------------------------------------------------------------------------

def build_v2_startup(database, user, db_field_size=128, user_field_size=64):
    """
    Build a Shentong-compatible startup packet.
    Shentong uses PG protocol v2.0 with enlarged fixed fields:
      proto(4) + database(128) + user(64) + options(64) + unused(64) + tty(64)
    Standard PG v2.0 uses database(64) + user(32).
    """
    proto = struct.pack("!I", 131072)  # v2.0
    db_field = database.encode()[:db_field_size - 1].ljust(db_field_size, b'\0')
    user_field = user.encode()[:user_field_size - 1].ljust(user_field_size, b'\0')
    body = proto + db_field + user_field + b'\0' * 64 + b'\0' * 64 + b'\0' * 64
    length = struct.pack("!I", 4 + len(body))
    return length + body


def pg_md5_password(user, password, salt):
    """Standard PostgreSQL MD5 password: 'md5' + md5(md5(password+user) + salt)."""
    step1 = hashlib.md5(password.encode() + user.encode()).hexdigest()
    return "md5" + hashlib.md5(step1.encode() + salt).hexdigest()


def send_password(sock, password_hash):
    """Send a PasswordMessage ('p') to the server."""
    payload = password_hash.encode() + b'\0'
    msg = b'p' + struct.pack("!I", 4 + len(payload)) + payload
    sock.sendall(msg)


def parse_error(data):
    """Decode a Shentong error message (GB2312/GBK encoded)."""
    return data.decode('gbk', errors='replace')


# ---------------------------------------------------------------------------
# Protocol probes
# ---------------------------------------------------------------------------

def probe_v3():
    """Test standard PostgreSQL v3.0 protocol (what psycopg2 uses)."""
    print("=== Probe: PostgreSQL v3.0 protocol ===")
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.settimeout(10)
    try:
        s.connect((HOST, PORT))
        params = b"user\0" + USER.encode() + b"\0database\0" + DBNAME.encode() + b"\0\0"
        proto = struct.pack("!I", 196608)  # v3.0
        length = struct.pack("!I", 4 + len(proto) + len(params))
        s.sendall(length + proto + params)
        resp = s.recv(4096)
        print(f"  Response: {parse_error(resp)}")
    except Exception as e:
        print(f"  Error: {e}")
    finally:
        s.close()
    print()


def probe_ssl():
    """Test SSL negotiation."""
    print("=== Probe: SSL negotiation ===")
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.settimeout(10)
    try:
        s.connect((HOST, PORT))
        s.sendall(struct.pack("!II", 8, 80877103))  # SSLRequest
        resp = s.recv(1)
        if resp == b'S':
            print("  SSL supported")
        elif resp == b'N':
            print("  SSL NOT supported")
        else:
            print(f"  Unexpected: {resp!r}")
    except socket.timeout:
        print("  Timed out (SSL not supported)")
    except Exception as e:
        print(f"  Error: {e}")
    finally:
        s.close()
    print()


def probe_v2_field_sizes():
    """Find the correct field sizes for v2.0 startup packet."""
    print("=== Probe: v2.0 field size variants ===")
    sizes = [(64, 32), (64, 64), (128, 32), (128, 64), (128, 128), (256, 64)]
    for db_sz, user_sz in sizes:
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.settimeout(5)
        try:
            s.connect((HOST, PORT))
            s.sendall(build_v2_startup(DBNAME, USER, db_sz, user_sz))
            resp = s.recv(4096)
            first = chr(resp[0]) if resp and resp[0] < 128 else "?"
            if resp and resp[0] == ord('R'):
                auth_type = struct.unpack("!I", resp[1:5])[0]
                print(f"  db({db_sz})+user({user_sz}): [R] Auth type={auth_type} -- ACCEPTED")
            else:
                err = parse_error(resp[:80]) if resp else "(empty)"
                print(f"  db({db_sz})+user({user_sz}): [{first}] {err.strip()}")
        except Exception as e:
            print(f"  db({db_sz})+user({user_sz}): {e}")
        finally:
            s.close()
    print()


def attempt_login():
    """Attempt full login with the discovered protocol parameters."""
    print("=== Login attempt: v2.0 db(128)+user(64) with MD5 auth ===")
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.settimeout(10)
    try:
        s.connect((HOST, PORT))
        s.sendall(build_v2_startup(DBNAME, USER))
        resp = s.recv(4096)
        if not resp or resp[0] != ord('R'):
            print(f"  Unexpected: {resp!r}")
            return

        auth_type = struct.unpack("!I", resp[1:5])[0]
        print(f"  Server requests auth type: {auth_type}")

        if auth_type == 5:
            salt = resp[5:9]
            print(f"  MD5 salt: {salt.hex()}")
            pwd_hash = pg_md5_password(USER, PASSWORD, salt)
            send_password(s, pwd_hash)
            resp2 = s.recv(4096)
            if resp2 and resp2[0] == ord('R') and struct.unpack("!I", resp2[1:5])[0] == 0:
                print("  *** AUTHENTICATION SUCCESS ***")
                # Read post-auth server parameters
                remaining = resp2[5:]
                try:
                    more = s.recv(4096)
                    remaining += more
                except socket.timeout:
                    pass
                print(f"  Post-auth data ({len(remaining)} bytes)")
                run_queries_raw(s)
            else:
                err = parse_error(resp2) if resp2 else "(empty)"
                print(f"  Auth failed: {err[:100]}")
                print(f"  (Password '{PASSWORD}' may be incorrect for user '{USER}')")
        elif auth_type == 3:
            print("  Cleartext password requested")
            send_password(s, PASSWORD)
            resp2 = s.recv(4096)
            if resp2 and resp2[0] == ord('R') and struct.unpack("!I", resp2[1:5])[0] == 0:
                print("  *** AUTHENTICATION SUCCESS ***")
                run_queries_raw(s)
            else:
                print(f"  Auth failed: {parse_error(resp2)[:100]}")
        elif auth_type == 0:
            print("  No authentication required!")
            run_queries_raw(s)
        else:
            print(f"  Unknown auth type: {auth_type}")

    except Exception as e:
        print(f"  Error: {e}")
    finally:
        s.close()
    print()


def try_psycopg2():
    """Attempt psycopg2 connection (expected to fail)."""
    print("=== psycopg2 attempt (expected to fail) ===")
    try:
        import psycopg2
        conn = psycopg2.connect(
            host=HOST, port=PORT, user=USER, password=PASSWORD,
            dbname=DBNAME, connect_timeout=15, sslmode="disable",
        )
        conn.autocommit = True
        print("  Connected! (unexpected)")
        cur = conn.cursor()
        for i, q in enumerate(QUERIES, 1):
            print(f"\n  --- Query {i}: {q}")
            try:
                cur.execute(q)
                if cur.description:
                    cols = [d[0] for d in cur.description]
                    rows = cur.fetchall()
                    print(f"  Columns: {cols}")
                    for row in rows[:20]:
                        print(f"  {row}")
            except Exception as e:
                print(f"  ERROR: {e}")
        cur.close()
        conn.close()
    except ImportError:
        print("  psycopg2 not installed")
    except Exception as e:
        print(f"  FAILED (as expected): {e}")
    print()


# ---------------------------------------------------------------------------
# Query execution over raw socket
# ---------------------------------------------------------------------------

def run_queries_raw(sock):
    """Execute queries over an authenticated raw socket using PG simple query protocol."""
    for i, query in enumerate(QUERIES, 1):
        print(f"\n  --- Query {i}: {query}")
        try:
            q_bytes = query.encode('utf-8') + b'\0'
            sock.sendall(b'Q' + struct.pack("!I", 4 + len(q_bytes)) + q_bytes)

            full_resp = b''
            while True:
                chunk = sock.recv(8192)
                if not chunk:
                    break
                full_resp += chunk
                if b'Z' in chunk:
                    break

            # Parse PG wire protocol messages
            idx = 0
            while idx < len(full_resp) - 5:
                msg_type = chr(full_resp[idx])
                msg_len = struct.unpack("!I", full_resp[idx + 1:idx + 5])[0]
                body = full_resp[idx + 5:idx + 1 + msg_len]

                if msg_type == 'T':
                    n = struct.unpack("!H", body[:2])[0]
                    pos, names = 2, []
                    for _ in range(n):
                        end = body.index(0, pos)
                        names.append(body[pos:end].decode('utf-8', errors='replace'))
                        pos = end + 1 + 18
                    print(f"    Columns ({n}): {names}")
                elif msg_type == 'D':
                    n = struct.unpack("!H", body[:2])[0]
                    vals, pos = [], 2
                    for _ in range(n):
                        vl = struct.unpack("!i", body[pos:pos + 4])[0]
                        pos += 4
                        if vl == -1:
                            vals.append(None)
                        else:
                            vals.append(body[pos:pos + vl].decode('utf-8', errors='replace'))
                            pos += vl
                    print(f"    Row: {vals}")
                elif msg_type == 'C':
                    print(f"    Complete: {body.decode('utf-8', errors='replace').rstrip(chr(0))}")
                elif msg_type == 'E':
                    print(f"    Error: {parse_error(body)[:120]}")

                idx += 1 + msg_len
        except Exception as e:
            print(f"    Exception: {e}")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    print(f"Target: Shentong OSCAR at {HOST}:{PORT}, user={USER}, db={DBNAME}\n")

    probe_v3()
    probe_ssl()
    probe_v2_field_sizes()
    attempt_login()
    try_psycopg2()

    print("=" * 60)
    print("SUMMARY:")
    print("  - PG v3.0 protocol: REJECTED by Shentong")
    print("  - SSL: NOT supported (timeout)")
    print("  - PG v2.0 with db(128)+user(64): ACCEPTED (auth reached)")
    print("  - psycopg2: CANNOT connect (uses v3.0 only)")
    print("  - Use ODBC (OSCAR ODBC DRIVER) for production access")
    print("=" * 60)


if __name__ == "__main__":
    main()
