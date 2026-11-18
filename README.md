# open-in-grayjay

Open YouTube videos in Grayjay Desktop from Firefox or Floorp using a small WebExtension and a Rust native messaging host.

Enable **Synchronization Enabled** and **Bind Listener** in Grayjay's Sync settings. Run `open-in-grayjay pair`, then paste the printed link into Grayjay Desktop → Sync → add device manually. Pairing is inverted because the desktop's responder mishandles pairing codes. Only YouTube video URLs are supported for now.

The native messaging host is named `open_in_grayjay`; it must be installed with a browser native-host manifest pointing to the `open-in-grayjay` executable and permitting `open-in-grayjay@tartanoglu.com`. Nix packaging and installation are not included yet. `open-in-grayjay open URL` also works directly. Use `--addr 127.0.0.1:PORT` or `GRAYJAY_ADDR` to override the Sync listener, and `GRAYJAY_COMMAND` to override the executable used to launch Grayjay.

The extension is unsigned: release Firefox requires AMO signing; Floorp, Firefox Developer Edition and ESR can disable signature checks where supported.

Licensed under MIT or Apache-2.0.
