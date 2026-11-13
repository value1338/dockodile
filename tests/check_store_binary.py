"""Release guard: Store builds must not use the purchase-gated web updater."""
import pathlib
import sys
binary = pathlib.Path(sys.argv[1]).read_bytes()
assert binary[:2] == b'MZ', 'Not a Windows binary'
for endpoint in (b'/api/version?platform=windows', b'/api/app/update'):
    assert endpoint not in binary, f'Store binary contains web updater endpoint: {endpoint!r}'
assert b'ms-windows-store://downloadsandupdates' in binary, 'Store update destination missing'
print('PASS: Store update destination present; web updater endpoints absent')
