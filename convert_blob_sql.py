
import re, sys

# Read the original SQL from stdin or file
if len(sys.argv) > 1:
    with open(sys.argv[1], 'r', encoding='utf-8') as f:
        sql = f.read()
else:
    sql = sys.stdin.read()

# Extract all HEXTORAW chunks from WRITEAPPEND calls
chunks = re.findall(r"HEXTORAW\('([0-9A-Fa-f]+)'\)", sql)

if len(chunks) < 2:
    print("ERROR: Expected at least 2 HEXTORAW chunks, found", len(chunks))
    sys.exit(1)

# Extract the INSERT template (everything before EMPTY_BLOB)
# Build new SQL
pk_id = "55864e82-0633-11f1-aa60-f61b81e13a43"

print("-- ShenTong large BLOB: pure SQL (no PL/SQL DECLARE block)")
print("-- Step 1: INSERT with first chunk inline")
print(f'INSERT INTO "act_ge_bytearray" ("id_", "rev_", "name_", "deployment_id_", "bytes_", "generated_") VALUES (\'{pk_id}\', 1, \'dt.bpmn20.xml\', \'55864e81-0633-11f1-aa60-f61b81e13a43\', TO_BLOB(HEXTORAW(\'{chunks[0]}\')), 0);')
print()
for i, chunk in enumerate(chunks[1:], 2):
    print(f"-- Step {i}: UPDATE append chunk {i}")
    print(f'UPDATE "act_ge_bytearray" SET "bytes_" = "bytes_" || TO_BLOB(HEXTORAW(\'{chunk}\')) WHERE "id_" = \'{pk_id}\';')
    print()
