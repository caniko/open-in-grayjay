# open-in-grayjay

<!-- simit:badges:start -->

[![CI](https://img.shields.io/badge/CI-managed-2088ff)](.github/workflows/ci.yaml)

<!-- simit:badges:end -->

Open YouTube videos in Grayjay Desktop from Firefox or Floorp. A small WebExtension hands the link to a Rust native messaging host, which asks Grayjay to open it over Grayjay's own Sync protocol, the same way Grayjay's phone app sends a video to the desktop.

## Install

The flake builds:

| Output | Contents |
|---|---|
| `host` | the `open-in-grayjay` binary |
| `native-host` | `lib/mozilla/native-messaging-hosts/open_in_grayjay.json`, pointing at the binary |
| `addon` | `share/mozilla/extensions/{ec8030f7-c20a-464f-9b0e-13a3a9e97384}/open-in-grayjay@tartanoglu.com.xpi` |
| `website` | the project site |

With Home Manager (use the same browser for both):

```nix
home.packages = [open-in-grayjay.packages.${system}.host]; # for `open-in-grayjay pair`
programs.floorp = {
  nativeMessagingHosts = [open-in-grayjay.packages.${system}.native-host];
  profiles.default.extensions.packages = [open-in-grayjay.packages.${system}.addon];
};
```

Without Home Manager, link the manifest where the browser looks for native hosts (`~/.mozilla/native-messaging-hosts/` for Firefox, `~/.floorp/native-messaging-hosts/` for Floorp) and install the `.xpi` from `about:addons`.

The extension is unsigned: release Firefox only installs AMO-signed add-ons; Floorp, Firefox Developer Edition and ESR can disable signature checks.

## Pair with Grayjay

1. In Grayjay Desktop's Sync settings, turn on **Synchronization Enabled** and **Bind Listener**, and turn off **Relay Pairing** (otherwise Grayjay tries the relay first and gives up before trying this machine). Restart Grayjay after saving: it reads these settings when Sync starts.
2. Run `open-in-grayjay pair` (or `nix run github:caniko/open-in-grayjay#host -- pair`).
3. Paste the printed `grayjay://sync/…` link into Grayjay's add-device dialog ("Paste device info here").

Pairing runs in this direction because Grayjay pairs reliably when it initiates the connection.

## Use

Right-click a YouTube link, or click the toolbar button on a YouTube video page, and choose **Open in Grayjay**. Only YouTube video URLs are supported for now. `open-in-grayjay open URL` works from a shell too.

If Grayjay isn't running, the host starts it and asks you to click again once its window has loaded: Grayjay accepts the request before its window can act on it.

`--addr 127.0.0.1:PORT` or `GRAYJAY_ADDR` overrides Grayjay's Sync listener (default port 12315); `GRAYJAY_COMMAND` overrides the command used to start Grayjay.

Licensed under MIT or Apache-2.0.
