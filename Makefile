# === Configuration ===

TARGET_NAME := lice

CC := clang

# --- Flags ---

INCLUDES := -Iinclude -Ivendor/fluf/include

CFLAGS := -g -Wall -Wextra -std=c23 -D_POSIX_C_SOURCE=200809L -D_DEFAULT_SOURCE $(INCLUDES)
CPPFLAGS := -MMD -MP

LDFLAGS :=
LDLIBS :=

# --- Dependencies (Fluf) ---

FLUF_DIR := vendor/fluf
FLUF_LIB := $(FLUF_DIR)/build/lib/libfluf.a

# 链接 fluf
LDFLAGS += -L$(FLUF_DIR)/build/lib
LDLIBS += -lfluf

# === Directories ===

SRC_DIR := src
BUILD_DIR := build

BIN_DIR := $(BUILD_DIR)/bin
OBJ_DIR := $(BUILD_DIR)/obj

# === Files ===

SRCS := $(shell find $(SRC_DIR) -name '*.c')

OBJS := $(patsubst $(SRC_DIR)/%.c,$(OBJ_DIR)/src/%.o,$(SRCS))

TARGET := $(BIN_DIR)/$(TARGET_NAME)

DEPS := $(OBJS:.o=.d)

# === Installation ===

PREFIX ?= /usr/local
INSTALL_BIN := $(PREFIX)/bin

# === Recipes ===

.PHONY: all clean install uninstall update run

all: $(TARGET)

# --- Link ---
$(TARGET): $(OBJS) $(FLUF_LIB)
	@echo "[MAKE]   LD $@"
	@mkdir -p $(dir $@)
	$(CC) $(OBJS) $(LDFLAGS) $(LDLIBS) -o $@

# --- Compile ---
$(OBJ_DIR)/src/%.o: $(SRC_DIR)/%.c
	@echo "[MAKE]   CC $<"
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) $(CPPFLAGS) -c $< -o $@

# --- Fluf Submodule ---
$(FLUF_LIB):
	@echo "[MAKE]   Building Dependency: fluf"
	@$(MAKE) -C $(FLUF_DIR)

# === Utilities ===

clean:
	@echo "[MAKE]   CLEAN (lice)"
	@rm -rf $(BUILD_DIR)
	@echo "[MAKE]   CLEAN (fluf)"
	@$(MAKE) -C $(FLUF_DIR) clean

run: all
	@echo "[RUN]    $(TARGET)"
	@$(TARGET)

# === Installation (Safety Checks Added) ===

install: all
	@echo "[CHECK]  Checking Root Privileges..."
	@if [ "$$(id -u)" -ne 0 ]; then \
		echo "Error: 'make install' must be run as root (e.g., sudo make install)."; \
		exit 1; \
	fi
	@echo "[CHECK]  Verifying Target..."
	@if [ ! -f $(TARGET) ]; then \
		echo "Error: Target '$(TARGET)' not found. Build failed?"; \
		exit 1; \
	fi
	@echo "[MAKE]   INSTALL $(TARGET_NAME) -> $(INSTALL_BIN)"
	@mkdir -p $(INSTALL_BIN)
	@cp $(TARGET) $(INSTALL_BIN)/$(TARGET_NAME)
	@chmod +x $(INSTALL_BIN)/$(TARGET_NAME)

uninstall:
	@echo "[CHECK]  Checking Root Privileges..."
	@if [ "$$(id -u)" -ne 0 ]; then \
		echo "Error: 'make uninstall' must be run as root (e.g., sudo make uninstall)."; \
		exit 1; \
	fi
	@echo "[MAKE]   UNINSTALL $(TARGET_NAME)"
	@rm -f $(INSTALL_BIN)/$(TARGET_NAME)

# === Update (Safety Checks Added) ===

update:
	@echo "[MAKE]   UPDATE PROJECT"
	@if [ "$$(id -u)" -eq 0 ]; then \
		echo "Error: Do not run 'make update' as root (with sudo)."; \
		echo "This avoids messing up git file permissions."; \
		echo "Run 'make update' as a normal user, then 'sudo make install'."; \
		exit 1; \
	fi
	@echo "[GIT]    Pulling latest changes..."
	@git pull
	@echo "[GIT]    Updating submodules..."
	@git submodule update --init --recursive
	@echo "[MAKE]   Rebuilding..."
	@$(MAKE) all
	@echo "==> Update complete. Run 'sudo make install' to install."

# === Versioning Helpers ===

# 1. get the newest tag 
CURRENT_TAG := $(shell git describe --tags --abbrev=0 2>/dev/null || echo "v0.0.0")

# 2. parse tag version (`v0.3.0` -> MAJOR=0, MINOR=3, PATCH=0) 
VERSION_BITS := $(subst v,,$(CURRENT_TAG))
MAJOR := $(word 1,$(subst ., ,$(VERSION_BITS)))
MINOR := $(word 2,$(subst ., ,$(VERSION_BITS)))
PATCH := $(word 3,$(subst ., ,$(VERSION_BITS)))

# 3. calculate nex version 
NEXT_PATCH := $(shell echo $$(($(PATCH)+1)))
NEW_TAG := v$(MAJOR).$(MINOR).$(NEXT_PATCH)

# 4. default msg value
NOTE ?= Maintenance update

.PHONY: btag

btag:
	@if [ -n "$$(git status --porcelain)" ]; then \
        echo "Error: Working directory is not clean. Commit changes first."; \
        exit 1; \
    fi
	@echo "[RELEASE] Bumping Patch: $(CURRENT_TAG) -> $(NEW_TAG)"
	@echo "[MESSAGE] Release $(NEW_TAG): $(NOTE)"
	git tag -a $(NEW_TAG) -m "Release $(NEW_TAG): $(NOTE)"
	git push origin $(NEW_TAG)


ifneq ($(MAKECMDGOALS),clean)
-include $(DEPS)
endif
