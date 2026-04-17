from impacket.smbconnection import SMBConnection
import os
import sys

host = os.environ.get('SHENTONG_SMB_HOST')
user = os.environ.get('SHENTONG_SMB_USER')
password = os.environ.get('SHENTONG_SMB_PASSWORD')

if not host or not user or not password:
    sys.exit('Set SHENTONG_SMB_HOST, SHENTONG_SMB_USER, and SHENTONG_SMB_PASSWORD before running this script')

conn = SMBConnection(host, host, None, 445, timeout=10)
conn.login(user, password)

# Explore E:\ShenTong
print("E:\\ShenTong:")
try:
    files = conn.listPath('E$', '/ShenTong/*')
    for f in files:
        name = f.get_longname()
        if name not in ('.', '..'):
            print(f"  {name}")
except Exception as e:
    print(f"  [error: {e}]")

# Also check E:\ShenTong\drivers or similar
print("\nE:\\ShenTong\\bin (if exists):")
try:
    files = conn.listPath('E$', '/ShenTong/bin/*')
    for f in files:
        name = f.get_longname()
        if name not in ('.', '..'):
            print(f"  {name}")
except Exception as e:
    print(f"  [error: {e}]")

# Look for aci.dll
print("\nSearching for aci.dll recursively in E:\\ShenTong...")
def find_files(conn, share, path, pattern, results=[]):
    try:
        entries = conn.listPath(share, path + '/*')
        for f in entries:
            name = f.get_longname()
            if name in ('.', '..'):
                continue
            full = path + '/' + name
            if pattern.lower() in name.lower():
                results.append(full)
            if f.is_directory():
                find_files(conn, share, full, pattern, results)
    except:
        pass
    return results

results = find_files(conn, 'E$', '/ShenTong', 'aci.dll')
for r in results:
    print(f"  {r}")
