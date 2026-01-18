.PHONY: build install clean

build:
	cargo build --release

install: build
	install -m 755 target/release/serabut /usr/local/bin/
	install -m 755 target/release/serabutd /usr/local/bin/
	install -d -m 755 /var/lib/serabut
	id -u serabut &>/dev/null || useradd -r -s /sbin/nologin serabut
	chown serabut:serabut /var/lib/serabut
	install -m 644 deploy/serabutd.service /etc/systemd/system/
	systemctl daemon-reload

uninstall:
	systemctl stop serabutd || true
	systemctl disable serabutd || true
	rm -f /etc/systemd/system/serabutd.service
	rm -f /usr/local/bin/serabut
	rm -f /usr/local/bin/serabutd
	systemctl daemon-reload

clean:
	cargo clean
