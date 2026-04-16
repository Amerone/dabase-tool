import struct
import sys

def read_pe_imports(filename):
    with open(filename, 'rb') as f:
        data = f.read()

    # MZ header
    if data[:2] != b'MZ':
        print("Not a PE file")
        return

    # Get PE header offset
    pe_offset = struct.unpack_from('<I', data, 0x3C)[0]

    # PE signature
    if data[pe_offset:pe_offset+4] != b'PE\x00\x00':
        print("Invalid PE signature")
        return

    # COFF header
    machine = struct.unpack_from('<H', data, pe_offset+4)[0]
    num_sections = struct.unpack_from('<H', data, pe_offset+6)[0]
    opt_header_size = struct.unpack_from('<H', data, pe_offset+20)[0]

    # Optional header - PE32+ (64-bit) has magic 0x20B
    magic = struct.unpack_from('<H', data, pe_offset+24)[0]
    is_64bit = magic == 0x20B

    # Data directories
    if is_64bit:
        dd_offset = pe_offset + 24 + 112  # PE32+
    else:
        dd_offset = pe_offset + 24 + 96   # PE32

    # Import directory (index 1)
    import_rva = struct.unpack_from('<I', data, dd_offset + 8)[0]
    import_size = struct.unpack_from('<I', data, dd_offset + 12)[0]

    if import_rva == 0:
        print("No imports")
        return

    # Find section containing import directory
    sections_offset = pe_offset + 24 + opt_header_size
    sections = []
    for i in range(num_sections):
        sec_offset = sections_offset + i * 40
        name = data[sec_offset:sec_offset+8].rstrip(b'\x00').decode('ascii', errors='replace')
        vsize = struct.unpack_from('<I', data, sec_offset+8)[0]
        vaddr = struct.unpack_from('<I', data, sec_offset+12)[0]
        raw_size = struct.unpack_from('<I', data, sec_offset+16)[0]
        raw_offset = struct.unpack_from('<I', data, sec_offset+20)[0]
        sections.append((name, vaddr, vaddr+vsize, raw_offset, raw_size))

    def rva_to_offset(rva):
        for name, va_start, va_end, raw_start, raw_size in sections:
            if va_start <= rva < va_end:
                return raw_start + (rva - va_start)
        return None

    # Parse import directory
    imports = []
    cur = rva_to_offset(import_rva)
    if cur is None:
        print(f"Cannot resolve import RVA 0x{import_rva:X}")
        return

    while True:
        orig_first_thunk = struct.unpack_from('<I', data, cur)[0]
        timestamp = struct.unpack_from('<I', data, cur+4)[0]
        forwarder = struct.unpack_from('<I', data, cur+8)[0]
        name_rva = struct.unpack_from('<I', data, cur+12)[0]
        first_thunk = struct.unpack_from('<I', data, cur+16)[0]
        cur += 20

        if name_rva == 0 and first_thunk == 0:
            break

        name_off = rva_to_offset(name_rva)
        if name_off is None:
            continue

        dll_name = b''
        i = name_off
        while i < len(data) and data[i] != 0:
            dll_name += bytes([data[i]])
            i += 1
        dll_name = dll_name.decode('ascii', errors='replace')
        imports.append(dll_name)

    print(f"DLL imports ({len(imports)} DLLs):")
    for dll in sorted(imports):
        print(f"  {dll}")

filename = 'drivers/shentong/windows/aci.dll'
print(f"Analyzing: {filename}")
read_pe_imports(filename)
