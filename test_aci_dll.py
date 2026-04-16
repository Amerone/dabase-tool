import ctypes
import os

aci_dir = r'E:\self\tool-database\drivers\shentong\windows'
aci = ctypes.cdll.LoadLibrary(os.path.join(aci_dir, 'aci.dll'))

# Check for NLS-related functions
print("Checking NLS functions:")
nls_fns = ['ACINlsNameMap', 'ACINlsGetInfo', 'ACINlsCharSetConvert',
           'ACINlsEnvironmentVariableGet', 'ACINlsNumericInfoGet',
           'ACINlsCharSetIdToName', 'ACINlsCharSetNameToId']
for fn in nls_fns:
    try:
        getattr(aci, fn)
        print(f"  {fn}: EXISTS")
    except AttributeError:
        print(f"  {fn}: NOT FOUND")
