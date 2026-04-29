import os
import sys
import re

if len(sys.argv) != 2:
    print("Использование: python bump.py <новая_версия>")
    print("Пример: python bump.py 1.0.3")
    sys.exit(1)

new_version = sys.argv[1]

# Проверяем формат версии (semver)
if not re.match(r"^\d+\.\d+\.\d+(-[a-zA-Z0-9.]+)?$", new_version):
    print("Ошибка: Версия должна быть в формате semver (например, 1.0.3 или 1.0.3-beta)")
    sys.exit(1)

# Вычисляем versionCode для Android (например, 1.0.2 -> 10002)
clean_version = new_version.split('-')[0]
major, minor, patch = map(int, clean_version.split('.'))
version_code = major * 10000 + minor * 100 + patch

cargo_files = [
    "crates/taiga-egui/Cargo.toml",
    "crates/taiga-mycelium/Cargo.toml",
    "crates/taiga-node/Cargo.toml",
    "crates/taiga-resin/Cargo.toml"
]

gradle_file = "crates/taiga-egui/android/app/build.gradle.kts"

def bump_cargo(file_path, new_version):
    if not os.path.exists(file_path):
        print(f"Пропуск {file_path} (файл не найден)")
        return

    with open(file_path, 'r', encoding='utf-8') as f:
        content = f.read()

    # Обновляем версию самого пакета
    content = re.sub(r'^version\s*=\s*\".*?\"', f'version = "{new_version}"', content, flags=re.MULTILINE)
    
    # Обновляем версии локальных зависимостей (taiga-mycelium = { version = "1.0.2", path = ... })
    content = re.sub(r'(taiga-[a-z]+)\s*=\s*\{\s*version\s*=\s*\".*?\"', r'\1 = { version = "' + new_version + '"', content)

    with open(file_path, 'w', encoding='utf-8') as f:
        f.write(content)
    print(f"Обновлен {file_path} -> {new_version}")

def bump_gradle(file_path, new_version, version_code):
    if not os.path.exists(file_path):
        print(f"Пропуск {file_path} (файл не найден)")
        return
        
    with open(file_path, 'r', encoding='utf-8') as f:
        content = f.read()

    # Обновляем versionName
    content = re.sub(r'versionName\s*=\s*\".*?\"', f'versionName = "{new_version}"', content)
    # Обновляем versionCode
    content = re.sub(r'versionCode\s*=\s*\d+', f'versionCode = {version_code}', content)

    with open(file_path, 'w', encoding='utf-8') as f:
        f.write(content)
    print(f"Обновлен {file_path} -> {new_version} (versionCode: {version_code})")

print(f"=== Обновление версии до {new_version} ===")

for cargo_file in cargo_files:
    bump_cargo(cargo_file, new_version)

bump_gradle(gradle_file, new_version, version_code)

print("\n✅ Версии успешно обновлены!")
print("Не забудьте выполнить 'cargo check --workspace' для обновления Cargo.lock.")
print("Для релиза сделайте коммит, а затем выполните команды:")
print(f"  git tag v{new_version}")
print(f"  git push origin v{new_version}")
