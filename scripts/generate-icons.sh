#!/bin/bash

rsvg-convert -w 1024 -h 1024 assets/eMterm.svg -o /tmp/eMterm-icon-1024.png
bun tauri icon /tmp/eMterm-icon-1024.png
