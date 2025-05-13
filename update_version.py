import json
import sys
import semver
import toml
from typing import Final

PACKAGE_JSON: Final = 'package.json'
PACKAGE_JSON_LOCK: Final = 'package-lock.json'
APP_CONFIG_LOCK: Final = 'src-tauri/tauri.conf.json'
APP_RS_TOML: Final = 'src-tauri/Cargo.toml'
APP_RS_LOCK: Final = 'src-tauri/Cargo.lock'

def bump_version(version, bump_type):
    if bump_type == 'major':
        return version.bump_major()
    elif bump_type == 'minor':
        return version.bump_minor()
    else:
        return version.bump_patch()

def update_version_toml(toml_path, bump_type):
    try:
        data = toml.load(toml_path)
    except Exception as e:
        print(f"Failed to load TOML from {toml_path}: {e}")
        return

    if "package" not in data or "version" not in data["package"]:
        print(f"No [package] version field found in {toml_path}")
        return

    old_version = data["package"]["version"]

    try:
        parsed_version = semver.Version.parse(old_version)
    except ValueError:
        print(f"Invalid version format: {old_version}")
        return

    new_version = bump_version(parsed_version, bump_type)
    data["package"]["version"] = str(new_version)

    try:
        with open(toml_path, "w") as f:
            toml.dump(data, f)
        print(f"Updated version {toml_path}: {old_version} → {new_version}")
    except Exception as e:
        print(f"Failed to write updated TOML to {toml_path}: {e}")

def update_version(json_path, bump_type):
    with open(json_path, 'r') as f:
        data = json.load(f)

    old_version = data.get('version')
    if not old_version:
        print("No 'version' field found.")
        return

    try:
        parsed_version = semver.Version.parse(old_version)
    except ValueError:
        print(f"Invalid version format: {old_version}")
        return

    new_version = bump_version(parsed_version, bump_type)

    data['version'] = str(new_version)

    with open(json_path, 'w') as f:
        json.dump(data, f, indent=2)

    print(f"Updated version {json_path}: {old_version} → {new_version}")

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python update_version.py [patch|minor|major]")
    else:
        bump_type = sys.argv[1] if len(sys.argv) > 1 else 'patch'
        update_version(PACKAGE_JSON, bump_type)
        update_version(PACKAGE_JSON_LOCK, bump_type)
        update_version(APP_CONFIG_LOCK, bump_type)
        update_version_toml(APP_RS_TOML, bump_type)
        update_version_toml(APP_RS_LOCK, bump_type)

