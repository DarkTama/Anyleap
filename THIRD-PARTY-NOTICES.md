# Third-Party Notices

AnyLeap is distributed under the Apache License 2.0 (see [LICENSE](LICENSE)).

Release builds of AnyLeap bundle the following third-party software, which is
**not** part of this repository's source (it is downloaded at build time by
`scripts/fetch-binaries.ps1` and packaged into the installer). Each component is
distributed under its own license; copies/attributions are reproduced below.

---

## scrcpy

- **Project:** scrcpy — <https://github.com/Genymobile/scrcpy>
- **Copyright:** © Genymobile / Romain Vimont and contributors
- **License:** Apache License 2.0
- **Bundled files:** `scrcpy.exe`, `scrcpy-server`, `scrcpy.png`,
  `disconnected.png`, and the SDL3 / FFmpeg / dav1d / libusb runtime DLLs
  shipped in the official `scrcpy-win64` release.

## Android SDK Platform-Tools (adb)

- **Project:** Android SDK Platform-Tools —
  <https://developer.android.com/tools/releases/platform-tools>
- **Copyright:** © The Android Open Source Project
- **License:** Apache License 2.0
- **Bundled files:** `adb.exe`, `AdbWinApi.dll`, `AdbWinUsbApi.dll`
  (shipped inside the scrcpy Windows release).

---

A copy of the Apache License 2.0 (the same license AnyLeap uses) applies to the
components above and is available in [LICENSE](LICENSE) and at
<https://www.apache.org/licenses/LICENSE-2.0>.
