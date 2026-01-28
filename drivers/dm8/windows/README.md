# DM8 Windows Driver Placeholder

Place DM8 ODBC driver binaries here for Windows builds:
- `dmodbc.dll` (primary driver)
- Any companion DLLs the driver depends on

The Tauri bundle will include everything under `drivers/dm8/`, so Windows builds must copy the real DM8 driver files into this folder before packaging.
