#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Detect CLI build mode. EMTERM_CLI_ONLY=1 → emterm-cli deb (Depends:
# libc6 only). The CLI binary still ships the mux daemon / bridge / CLI
# plus the markdown / json / yaml / image subcommands; only the GUI
# (winit / wgpu / wry / GTK / WebKitGTK) is dropped.
CLI_ONLY="${EMTERM_CLI_ONLY:-}"

if [ -n "$CLI_ONLY" ]; then
    echo -e "${GREEN}Building CLI dpkg package for emterm...${NC}"
else
    echo -e "${GREEN}Building dpkg package for emterm...${NC}"
fi

# Get project information
PROJECT_NAME="emterm"
VERSION=$(git describe --tags --always 2>/dev/null | sed 's/^v//' || echo "0.1.0")
ARCH=$(uname -m)
MAINTAINER="m-m-n <51132276+m-m-n@users.noreply.github.com>"

# Convert architecture to Debian format
case "$ARCH" in
    x86_64)
        DEB_ARCH="amd64"
        RUST_TARGET="x86_64-unknown-linux-gnu"
        ;;
    aarch64)
        DEB_ARCH="arm64"
        RUST_TARGET="aarch64-unknown-linux-gnu"
        ;;
    armv7l)
        DEB_ARCH="armhf"
        RUST_TARGET="armv7-unknown-linux-gnueabihf"
        ;;
    i686)
        DEB_ARCH="i386"
        RUST_TARGET="i686-unknown-linux-gnu"
        ;;
    *)
        DEB_ARCH="$ARCH"
        RUST_TARGET="$ARCH-unknown-linux-gnu"
        ;;
esac

if [ -n "$CLI_ONLY" ]; then
    DEB_PACKAGE="emterm-cli"
else
    DEB_PACKAGE="${PROJECT_NAME}"
fi
PACKAGE_NAME="${DEB_PACKAGE}_${VERSION}_${DEB_ARCH}"
BUILD_DIR="build/dpkg/${PACKAGE_NAME}"
CARGO_TARGET_HOST="src-tauri/target-host"
BINARY_PATH="${CARGO_TARGET_HOST}/release/${PROJECT_NAME}"

echo ""
echo -e "${BLUE}═══════════════════════════════════════${NC}"
echo -e "${YELLOW}Package: ${PACKAGE_NAME}${NC}"
echo -e "${YELLOW}Version: ${VERSION}${NC}"
echo -e "${YELLOW}Architecture: ${DEB_ARCH}${NC}"
echo -e "${YELLOW}Maintainer: ${MAINTAINER}${NC}"
if [ -n "$CLI_ONLY" ]; then
    echo -e "${YELLOW}Build Mode: CLI${NC}"
fi
echo -e "${BLUE}═══════════════════════════════════════${NC}"
echo ""

# Check if dpkg-deb is available
if ! command -v dpkg-deb &> /dev/null; then
    echo -e "${RED}Error: dpkg-deb command not found${NC}"
    echo "Please install dpkg tools: sudo apt-get install dpkg"
    exit 1
fi

# Clean previous build
if [ -d "build/dpkg" ]; then
    echo "Cleaning previous build..."
    rm -rf build/dpkg
fi

# Create directory structure
echo "Creating package directory structure..."
mkdir -p "${BUILD_DIR}/DEBIAN"
mkdir -p "${BUILD_DIR}/usr/bin"
mkdir -p "${BUILD_DIR}/usr/share/doc/${PROJECT_NAME}"
if [ -z "$CLI_ONLY" ]; then
    mkdir -p "${BUILD_DIR}/usr/share/applications"
    mkdir -p "${BUILD_DIR}/usr/share/icons/hicolor/32x32/apps"
    mkdir -p "${BUILD_DIR}/usr/share/icons/hicolor/128x128/apps"
    mkdir -p "${BUILD_DIR}/usr/share/icons/hicolor/256x256/apps"
fi

# Build the binary
if [ -n "$CLI_ONLY" ]; then
    echo "Building CLI binary..."
    if ! CARGO_TARGET_DIR="${CARGO_TARGET_HOST}" cargo build --manifest-path src-tauri/Cargo.toml --release --no-default-features; then
        echo -e "${RED}Failed to build CLI binary${NC}"
        exit 1
    fi
else
    echo "Building web bundles (viewer + settings)..."
    if ! bun run build:viewer; then
        echo -e "${RED}Failed to build Markdown viewer bundle${NC}"
        exit 1
    fi
    if ! bun run build:settings; then
        echo -e "${RED}Failed to build settings window bundle${NC}"
        exit 1
    fi
    echo "Generating app icons..."
    if ! bash scripts/generate-icons.sh; then
        echo -e "${RED}Failed to generate app icons${NC}"
        exit 1
    fi
    echo "Building emterm (GUI) binary..."
    if ! CARGO_TARGET_DIR="${CARGO_TARGET_HOST}" cargo build --manifest-path src-tauri/Cargo.toml --release; then
        echo -e "${RED}Failed to build emterm binary${NC}"
        exit 1
    fi
fi

# Verify binary exists
if [ ! -f "${BINARY_PATH}" ]; then
    echo -e "${RED}Error: Binary ${BINARY_PATH} not found after build${NC}"
    exit 1
fi

# Copy binary
echo "Copying binary to package..."
cp "${BINARY_PATH}" "${BUILD_DIR}/usr/bin/"
chmod 755 "${BUILD_DIR}/usr/bin/${PROJECT_NAME}"

# Strip binary for smaller size
if command -v strip &> /dev/null; then
    echo "Stripping binary..."
    strip "${BUILD_DIR}/usr/bin/${PROJECT_NAME}"
fi

# Copy documentation
echo "Copying documentation..."
if [ -f "README.md" ]; then
    cp README.md "${BUILD_DIR}/usr/share/doc/${PROJECT_NAME}/"
    chmod 644 "${BUILD_DIR}/usr/share/doc/${PROJECT_NAME}/README.md"
fi

if [ -f "LICENSE" ]; then
    cp LICENSE "${BUILD_DIR}/usr/share/doc/${PROJECT_NAME}/copyright"
    chmod 644 "${BUILD_DIR}/usr/share/doc/${PROJECT_NAME}/copyright"
elif [ -f "LICENCE" ]; then
    cp LICENCE "${BUILD_DIR}/usr/share/doc/${PROJECT_NAME}/copyright"
    chmod 644 "${BUILD_DIR}/usr/share/doc/${PROJECT_NAME}/copyright"
fi

if [ -z "$CLI_ONLY" ]; then
    # Copy icons (GUI only)
    echo "Copying icons..."
    if [ -f "src-tauri/icons/32x32.png" ]; then
        cp "src-tauri/icons/32x32.png" "${BUILD_DIR}/usr/share/icons/hicolor/32x32/apps/${PROJECT_NAME}.png"
    fi
    if [ -f "src-tauri/icons/128x128.png" ]; then
        cp "src-tauri/icons/128x128.png" "${BUILD_DIR}/usr/share/icons/hicolor/128x128/apps/${PROJECT_NAME}.png"
    fi
    if [ -f "src-tauri/icons/128x128@2x.png" ]; then
        cp "src-tauri/icons/128x128@2x.png" "${BUILD_DIR}/usr/share/icons/hicolor/256x256/apps/${PROJECT_NAME}.png"
    fi

    # Create desktop file (GUI only)
    echo "Creating desktop file..."
    cat > "${BUILD_DIR}/usr/share/applications/${PROJECT_NAME}.desktop" << EOF
[Desktop Entry]
Name=eMterm
Comment=Native terminal emulator with rich rendering capabilities
Exec=${PROJECT_NAME}
Icon=${PROJECT_NAME}
Terminal=false
Type=Application
Categories=System;TerminalEmulator;
Keywords=terminal;console;shell;
StartupWMClass=emterm
EOF
    chmod 644 "${BUILD_DIR}/usr/share/applications/${PROJECT_NAME}.desktop"
fi

# Create changelog
echo "Creating changelog..."
cat > "${BUILD_DIR}/usr/share/doc/${PROJECT_NAME}/changelog" << EOF
${PROJECT_NAME} (${VERSION}) stable; urgency=low

  * Release version ${VERSION}
  * See git history for detailed changes

 -- ${MAINTAINER}  $(date -R)
EOF
chmod 644 "${BUILD_DIR}/usr/share/doc/${PROJECT_NAME}/changelog"

# Compress changelog
if command -v gzip &> /dev/null; then
    gzip -9 "${BUILD_DIR}/usr/share/doc/${PROJECT_NAME}/changelog"
fi

# Create DEBIAN/control file
echo "Creating control file..."
if [ -n "$CLI_ONLY" ]; then
    cat > "${BUILD_DIR}/DEBIAN/control" << 'EOF'
Package: emterm-cli
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${DEB_ARCH}
Maintainer: m-m-n <51132276+m-m-n@users.noreply.github.com>
Depends: libc6
Description: CLI + mux daemon for eMterm terminal emulator
 Command-line tools for displaying images, Markdown, and structured
 data in compatible terminal emulators, plus the eMterm mux daemon
 and bridge for headless SSH hosts. No GUI dependencies.
 .
 Commands:
  - emterm image: Display images via Kitty/SIXEL protocol
  - emterm markdown: Display Markdown via OSC extension
  - emterm json: Display JSON via OSC extension
  - emterm yaml: Display YAML via OSC extension
  - emterm mux: start a mux session (auto-spawns the daemon)
  - emterm mux --daemon: run the eMterm mux daemon in the foreground
  - emterm mux attach: bridge into a running daemon
EOF
else
    cat > "${BUILD_DIR}/DEBIAN/control" << 'EOF'
Package: emterm
Version: ${VERSION}
Section: x11
Priority: optional
Architecture: ${DEB_ARCH}
Maintainer: m-m-n <51132276+m-m-n@users.noreply.github.com>
Depends: libc6, libwebkit2gtk-4.1-0, libgtk-3-0, libglib2.0-0
Description: Native terminal emulator with rich rendering capabilities
 A modern terminal emulator with a native wgpu+swash render pipeline,
 inline image protocols (Kitty / SIXEL), and child WebView windows for
 Markdown / JSON / YAML viewing and the settings panel.
 .
 Features:
  - Full ANSI control sequence support
  - Kitty Graphics Protocol / SIXEL for inline images
  - Custom OSC extension for Markdown / JSON / YAML rendering
  - Low-latency typing performance
  - tmux-style multiplexing (windows / tabs / panes)
EOF
fi

# Substitute variables in control file
sed -i "s/\${VERSION}/${VERSION}/g" "${BUILD_DIR}/DEBIAN/control"
sed -i "s/\${DEB_ARCH}/${DEB_ARCH}/g" "${BUILD_DIR}/DEBIAN/control"

if [ -z "$CLI_ONLY" ]; then
    # Create postinst script (GUI only)
    echo "Creating postinst script..."
    cat > "${BUILD_DIR}/DEBIAN/postinst" << 'EOF'
#!/bin/bash
set -e

# Update icon cache
if command -v gtk-update-icon-cache &> /dev/null; then
    gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
fi

# Update desktop database
if command -v update-desktop-database &> /dev/null; then
    update-desktop-database /usr/share/applications 2>/dev/null || true
fi

echo "emterm installed successfully!"
echo "Run 'emterm' to start the terminal emulator."

exit 0
EOF
    chmod 755 "${BUILD_DIR}/DEBIAN/postinst"

    # Create prerm script (GUI only)
    echo "Creating prerm script..."
    cat > "${BUILD_DIR}/DEBIAN/prerm" << 'EOF'
#!/bin/bash
set -e

# Clean up before removal (if needed)

exit 0
EOF
    chmod 755 "${BUILD_DIR}/DEBIAN/prerm"

    # Create postrm script (GUI only)
    echo "Creating postrm script..."
    cat > "${BUILD_DIR}/DEBIAN/postrm" << 'EOF'
#!/bin/bash
set -e

# Update icon cache
if command -v gtk-update-icon-cache &> /dev/null; then
    gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
fi

# Update desktop database
if command -v update-desktop-database &> /dev/null; then
    update-desktop-database /usr/share/applications 2>/dev/null || true
fi

echo "emterm has been removed."

exit 0
EOF
    chmod 755 "${BUILD_DIR}/DEBIAN/postrm"
fi

# Set proper permissions
echo "Setting file permissions..."
find "${BUILD_DIR}/usr/share/doc" -type f -exec chmod 644 {} \;
find "${BUILD_DIR}/usr/share/doc" -type d -exec chmod 755 {} \;
if [ -z "$CLI_ONLY" ]; then
    find "${BUILD_DIR}/usr/share/icons" -type f -exec chmod 644 {} \;
    find "${BUILD_DIR}/usr/share/icons" -type d -exec chmod 755 {} \;
fi

# Calculate installed size (in KB)
INSTALLED_SIZE=$(du -sk "${BUILD_DIR}" | cut -f1)
echo "Installed-Size: ${INSTALLED_SIZE}" >> "${BUILD_DIR}/DEBIAN/control"

# Build the package
echo ""
echo -e "${BLUE}Building .deb package...${NC}"
if dpkg-deb --build --root-owner-group "${BUILD_DIR}"; then
    # Move the package to build directory
    mkdir -p build
    mv "build/dpkg/${PACKAGE_NAME}.deb" build/

    echo ""
    echo -e "${GREEN}═══════════════════════════════════════${NC}"
    echo -e "${GREEN}Package created successfully!${NC}"
    echo -e "${GREEN}═══════════════════════════════════════${NC}"
    echo ""
    echo -e "${BLUE}Package file: ${YELLOW}build/${PACKAGE_NAME}.deb${NC}"

    # Show package info
    echo ""
    echo -e "${BLUE}Package Information:${NC}"
    dpkg-deb --info "build/${PACKAGE_NAME}.deb" | head -20

    echo ""
    echo -e "${BLUE}Package Contents:${NC}"
    dpkg-deb --contents "build/${PACKAGE_NAME}.deb"

    echo ""
    echo -e "${GREEN}Installation Commands:${NC}"
    echo -e "  ${YELLOW}sudo dpkg -i build/${PACKAGE_NAME}.deb${NC}     - Install package"
    echo -e "  ${YELLOW}dpkg -L ${DEB_PACKAGE}${NC}                        - List installed files"
    echo -e "  ${YELLOW}sudo dpkg -r ${DEB_PACKAGE}${NC}                   - Remove package"
    echo -e "  ${YELLOW}sudo dpkg -P ${DEB_PACKAGE}${NC}                   - Purge package completely"
    echo ""
else
    echo -e "${RED}Failed to build package${NC}"
    exit 1
fi

# Clean up build directory
echo "Cleaning up build directory..."
rm -rf build/dpkg

echo -e "${GREEN}Done!${NC}"
