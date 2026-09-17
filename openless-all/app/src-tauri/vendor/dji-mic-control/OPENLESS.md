# Embedded DJI Mic support

Source: https://github.com/ShadowBitBasher/DJI-Mic-Control
Release: 1.1.0, commit 9ba76880807a71d4eaba74c785dbee186a98f43b.
License: Unlicense (see LICENSE).

Only the protocol and USB device library crates are embedded. No external GUI,
tray application, CLI, updater or driver installer is bundled or started.
OpenLess uses the receiver vendor control interface (MI_06), leaving audio and
HID interfaces untouched. Battery is a seven-level gauge, not a percentage.

Local changes: trimmed workspace members/dependencies; bounded setting-response
wait to three seconds. USB write completion is not a device acknowledgement;
OpenLess confirms settings through subsequent device status reports.
