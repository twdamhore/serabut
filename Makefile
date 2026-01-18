.PHONY: all build release test coverage install uninstall clean

BINARY_NAME = serabutd
INSTALL_DIR = /usr/local/bin
CONFIG_DIR = /etc
DATA_DIR = /var/lib/serabutd/config
SYSTEMD_DIR = /etc/systemd/system
LOGROTATE_DIR = /etc/logrotate.d
LOG_DIR = /var/log/serabut

all: release

build:
	cargo build

release:
	cargo build --release

test:
	cargo test

coverage:
	cargo tarpaulin --out Html --output-dir coverage

install:
	@if [ ! -f target/release/$(BINARY_NAME) ]; then \
		echo "Error: Binary not found. Run 'make release' first."; \
		exit 1; \
	fi
	@echo "Installing $(BINARY_NAME)..."
	install -Dm755 target/release/$(BINARY_NAME) $(INSTALL_DIR)/$(BINARY_NAME)
	@echo "Installing systemd service..."
	install -Dm644 deploy/serabutd.service $(SYSTEMD_DIR)/serabutd.service
	@echo "Creating config directory..."
	install -dm755 $(DATA_DIR)
	install -dm755 $(DATA_DIR)/hardware
	install -dm755 $(DATA_DIR)/iso
	@if [ ! -f $(CONFIG_DIR)/serabutd.conf ]; then \
		echo "Installing default config..."; \
		install -Dm644 deploy/serabutd.conf $(CONFIG_DIR)/serabutd.conf; \
	else \
		echo "Config file already exists, skipping..."; \
	fi
	@if [ ! -f $(DATA_DIR)/action.cfg ]; then \
		echo "Creating empty action.cfg..."; \
		touch $(DATA_DIR)/action.cfg; \
	fi
	@echo "Creating log directory..."
	install -dm755 $(LOG_DIR)
	@echo "Installing logrotate config..."
	install -Dm644 deploy/serabutd.logrotate $(LOGROTATE_DIR)/serabutd
	@echo "Reloading systemd..."
	systemctl daemon-reload
	@echo "Installation complete!"
	@echo ""
	@echo "To start the service:"
	@echo "  sudo systemctl start serabutd"
	@echo "  sudo systemctl enable serabutd"

uninstall:
	@echo "Stopping service..."
	-systemctl stop serabutd
	-systemctl disable serabutd
	@echo "Removing files..."
	rm -f $(INSTALL_DIR)/$(BINARY_NAME)
	rm -f $(SYSTEMD_DIR)/serabutd.service
	rm -f $(LOGROTATE_DIR)/serabutd
	systemctl daemon-reload
	@echo "Uninstall complete!"
	@echo "Note: Config, data, and log files were preserved."

clean:
	cargo clean
	rm -rf coverage
