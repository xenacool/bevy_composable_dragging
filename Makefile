EXAMPLES = hello toggle three_way reorder_list ring_of_beads slider clock scheduler lists_in_lists tessellations chain_of_cells blend_square tactile_tessellations gizmo_3d gradient_hanoi_3d gradient_kanban_2d cozy_castle kart_racer_editor physics_lab

# Examples that produce animated WebP (all except hello, which is a still WebP)
ANIMATIONS = toggle three_way reorder_list ring_of_beads slider clock scheduler lists_in_lists tessellations chain_of_cells blend_square tactile_tessellations gizmo_3d gradient_hanoi_3d gradient_kanban_2d physics_lab

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

chain_of_cells:
	cargo run --example chain_of_cells --features pbr

blend_square:
	cargo run --example blend_square --features pbr

tactile_tessellations:
	cargo run --example tactile_tessellations --features shapes-2d

gizmo_3d:
	cargo run --example gizmo_3d --features shapes-3d

gradient_hanoi_3d:
	cargo run --example gradient_hanoi_3d --features shapes-3d

gradient_kanban_2d:
	cargo run --example gradient_kanban_2d --features shapes-2d

cozy_castle:
	cargo run --example cozy_castle_3d --features shapes-3d

kart_racer_editor:
	cargo run --example kart_racer_editor --features shapes-3d

physics_lab:
	cargo run --example physics_lab --features shapes-3d

ANIMATIONS_WEBP = $(patsubst %,screenshot_%.webp,$(ANIMATIONS))

# Detect whether we need a virtual display for screenshot capture
XVFB_RUN := $(shell if [ -z "$$DISPLAY" ] && [ -z "$$WAYLAND_DISPLAY" ] && [ -z "$$WAYLAND_SOCKET" ] && command -v xvfb-run >/dev/null 2>&1; then echo "xvfb-run --auto-servernum --server-args='-screen 0 1280x720x24'"; fi)

# Arguments: example name, cargo features. The example may save the PNG to the project root
# or directly to screenshots/; either location is accepted. Some examples (cozy_castle,
# kart_racer_editor) produce a second "_menu" shot as well.
define capture_screenshot
	@echo "=== Capturing $(1) screenshot ==="
	$(XVFB_RUN) env DSD_SCREENSHOT=1 timeout 120 cargo run --example $(1) --features $(2) 2>&1 || true
	@if [ -f screenshots/screenshot_$(1).png ]; then \
		ffmpeg -y -i screenshots/screenshot_$(1).png -vcodec libwebp -quality 75 \
			screenshots/screenshot_$(1).webp 2>/dev/null && \
		echo "Saved screenshots/screenshot_$(1).webp" || \
		echo "Warning: ffmpeg WebP encoding failed for $(1)"; \
		rm -f screenshots/screenshot_$(1).png; \
	elif [ -f screenshot_$(1).png ]; then \
		ffmpeg -y -i screenshot_$(1).png -vcodec libwebp -quality 75 \
			screenshots/screenshot_$(1).webp 2>/dev/null && \
		echo "Saved screenshots/screenshot_$(1).webp" || \
		echo "Warning: ffmpeg WebP encoding failed for $(1)"; \
		rm -f screenshot_$(1).png; \
	fi
	@if [ -f screenshots/screenshot_$(1)_menu.png ]; then \
		ffmpeg -y -i screenshots/screenshot_$(1)_menu.png -vcodec libwebp -quality 75 \
			screenshots/screenshot_$(1)_menu.webp 2>/dev/null && \
		echo "Saved screenshots/screenshot_$(1)_menu.webp" || \
		echo "Warning: ffmpeg WebP encoding failed for $(1)_menu"; \
		rm -f screenshots/screenshot_$(1)_menu.png; \
	elif [ -f screenshot_$(1)_menu.png ]; then \
		ffmpeg -y -i screenshot_$(1)_menu.png -vcodec libwebp -quality 75 \
			screenshots/screenshot_$(1)_menu.webp 2>/dev/null && \
		echo "Saved screenshots/screenshot_$(1)_menu.webp" || \
		echo "Warning: ffmpeg WebP encoding failed for $(1)_menu"; \
		rm -f screenshot_$(1)_menu.png; \
	fi
endef

# Arguments: example name, cargo features. Frames are captured on a fixed 30fps clock, so they
# are encoded at 30fps and the drag plays back in real time. Frames are saved in the gitignored
# `frames/` directory.
define capture_animation
	@echo "=== Capturing $(1) animation ==="
	@mkdir -p frames
	$(XVFB_RUN) env DSD_SCREENSHOT=1 timeout 1800 cargo run --example $(1) --features $(2) 2>&1 || true
	@if ls frames/screenshot_$(1)_frame_*.png 1>/dev/null 2>&1; then \
		echo "Encoding frames to WebP..."; \
		ffmpeg -y -framerate 30 -i frames/screenshot_$(1)_frame_%04d.png \
			-vcodec libwebp -quality 75 -loop 0 \
			screenshots/screenshot_$(1).webp 2>/dev/null && \
		echo "Saved screenshots/screenshot_$(1).webp" || \
		echo "Warning: ffmpeg WebP encoding failed for $(1)"; \
		rm -f frames/screenshot_$(1)_frame_*.png; \
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
	$(call capture_animation,chain_of_cells,pbr)
	@echo ""
	$(call capture_animation,blend_square,pbr)
	@echo ""
	$(call capture_animation,tactile_tessellations,shapes-2d)
	@echo ""
	$(call capture_animation,gizmo_3d,shapes-3d)
	@echo ""
	$(call capture_animation,gradient_hanoi_3d,shapes-3d)
	@echo ""
	$(call capture_animation,gradient_kanban_2d,shapes-2d)
	@echo ""
	$(call capture_screenshot,cozy_castle,shapes-3d)
	@echo ""
	$(call capture_screenshot,kart_racer_editor,shapes-3d)
	@echo ""
	$(call capture_animation,physics_lab,shapes-3d)
	@echo ""
	@echo "=== Screenshots saved to screenshots/ ==="
	@ls -la screenshots/

clean:
	cargo clean
