EXAMPLES = hello toggle three_way reorder_list ring_of_beads slider clock scheduler lists_in_lists tessellations gradient_hanoi_3d gradient_kanban_2d

# Examples that produce animated WebP (all except hello, which is a still WebP)
ANIMATIONS = toggle three_way reorder_list ring_of_beads slider clock scheduler lists_in_lists tessellations gradient_hanoi_3d gradient_kanban_2d

.PHONY: $(EXAMPLES) all-examples screenshots clean

all-examples:
	cargo build --examples --features shapes-2d,shapes-3d

hello:
	cargo run --example hello --features pbr

toggle:
	cargo run --example toggle --features pbr

three_way:
	cargo run --example three_way --features pbr

reorder_list:
	cargo run --example reorder_list --features pbr

ring_of_beads:
	cargo run --example ring_of_beads --features pbr

slider:
	cargo run --example slider --features pbr

clock:
	cargo run --example clock --features pbr

scheduler:
	cargo run --example scheduler --features pbr

lists_in_lists:
	cargo run --example lists_in_lists --features pbr

tessellations:
	cargo run --example tessellations --features pbr

gradient_hanoi_3d:
	cargo run --example gradient_hanoi_3d --features shapes-3d

gradient_kanban_2d:
	cargo run --example gradient_kanban_2d --features shapes-2d

ANIMATIONS_WEBP = $(patsubst %,screenshot_%.webp,$(ANIMATIONS))

# Detect whether we need a virtual display for screenshot capture
XVFB_RUN := $(shell if [ -z "$$DISPLAY" ] && [ -z "$$WAYLAND_DISPLAY" ] && [ -z "$$WAYLAND_SOCKET" ] && command -v xvfb-run >/dev/null 2>&1; then echo "xvfb-run --auto-servernum --server-args='-screen 0 1280x720x24'"; fi)

# Arguments: example name, cargo features.
define capture_screenshot
	@echo "=== Capturing $(1) screenshot ==="
	$(XVFB_RUN) env DSD_SCREENSHOT=1 timeout 120 cargo run --example $(1) --features $(2) 2>&1 || true
	@if [ -f screenshot_$(1).png ]; then \
		ffmpeg -y -i screenshot_$(1).png -vcodec libwebp -quality 75 \
			screenshots/screenshot_$(1).webp 2>/dev/null && \
		echo "Saved screenshots/screenshot_$(1).webp" || \
		echo "Warning: ffmpeg WebP encoding failed for $(1)"; \
		rm -f screenshot_$(1).png; \
	fi
endef

# Arguments: example name, cargo features. Frames are captured on a fixed 30fps clock, so they
# are encoded at 30fps and the drag plays back in real time.
define capture_animation
	@echo "=== Capturing $(1) animation ==="
	$(XVFB_RUN) env DSD_SCREENSHOT=1 timeout 1800 cargo run --example $(1) --features $(2) 2>&1 || true
	@if ls screenshot_$(1)_frame_*.png 1>/dev/null 2>&1; then \
		echo "Encoding frames to WebP..."; \
		ffmpeg -y -framerate 30 -i screenshot_$(1)_frame_%04d.png \
			-vcodec libwebp -quality 75 -loop 0 \
			screenshots/screenshot_$(1).webp 2>/dev/null && \
		echo "Saved screenshots/screenshot_$(1).webp" || \
		echo "Warning: ffmpeg WebP encoding failed for $(1)"; \
		rm -f screenshot_$(1)_frame_*.png; \
	else \
		echo "Warning: no frames found for $(1)"; \
	fi
endef

screenshots:
	@echo "=== Building all examples ==="
	cargo build --examples --features shapes-2d,shapes-3d
	@mkdir -p screenshots
	@echo ""
	$(call capture_screenshot,hello,pbr)
	@echo ""
	$(call capture_animation,toggle,pbr)
	@echo ""
	$(call capture_animation,three_way,pbr)
	@echo ""
	$(call capture_animation,reorder_list,pbr)
	@echo ""
	$(call capture_animation,ring_of_beads,pbr)
	@echo ""
	$(call capture_animation,slider,pbr)
	@echo ""
	$(call capture_animation,clock,pbr)
	@echo ""
	$(call capture_animation,scheduler,pbr)
	@echo ""
	$(call capture_animation,lists_in_lists,pbr)
	@echo ""
	$(call capture_animation,tessellations,pbr)
	@echo ""
	$(call capture_animation,gradient_hanoi_3d,shapes-3d)
	@echo ""
	$(call capture_animation,gradient_kanban_2d,shapes-2d)
	@echo ""
	@echo "=== Screenshots saved to screenshots/ ==="
	@ls -la screenshots/

clean:
	cargo clean
