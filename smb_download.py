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

def download_file(share, remote_path, local_path):
    os.makedirs(os.path.dirname(local_path), exist_ok=True)
    with open(local_path, 'wb') as f:
        conn.getFile(share, remote_path, f.write)
    size = os.path.getsize(local_path)
    print(f"OK: {os.path.basename(local_path)} ({size} bytes)")

local_dir = 'drivers/shentong/windows'
os.makedirs(local_dir, exist_ok=True)

# Download aci/win64 DLLs (standalone ACI client)
for fname in ['aci.dll', 'pthreadVC2_x64.dll', 'ACCI.dll']:
    try:
        download_file('E$', f'/ShenTong/aci/win64/{fname}', f'{local_dir}/{fname}')
    except Exception as e:
        print(f"SKIP {fname}: {e}")

# Also check if ONET.dll needs to come along
# First check what the aci.dll bin directory contains for runtime deps
print("\nChecking E:\\ShenTong\\onet\\win64 or similar...")
import traceback
for path in ['/ShenTong/onet/win64', '/ShenTong/onet', '/ShenTong/bin']:
    try:
        files = conn.listPath('E$', path + '/*')
        dll_files = [f for f in files if f.get_longname().lower().endswith('.dll') and f.get_longname() not in ('.', '..')]
        if dll_files:
            print(f"\n{path} DLLs:")
            for f in dll_files:
                print(f"  {f.get_longname()} ({f.get_filesize()} bytes)")
    except Exception as e:
        pass

print("\nDone!")
