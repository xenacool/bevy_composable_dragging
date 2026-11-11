EXAMPLES = hello toggle three_way reorder_list ring_of_beads slider clock scheduler lists_in_lists tessellations chain_of_cells blend_square tactile_tessellations gizmo_3d gradient_hanoi_3d gradient_kanban_2d constellation dag cozy_castle kart_racer_editor physics_lab

# Examples that produce animated GIF (all except hello, which is a still GIF)
ANIMATIONS = toggle three_way reorder_list ring_of_beads slider clock scheduler lists_in_lists tessellations chain_of_cells blend_square tactile_tessellations gizmo_3d gradient_hanoi_3d gradient_kanban_2d constellation dag cozy_castle kart_racer_editor physics_lab

.PHONY: $(EXAMPLES) all-examples screenshots clean cozy_castle_tour kart_racer_editor_tour

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

constellation:
	cargo run --example constellation --features gradmesh

dag:
	cargo run --example dag --features gradmesh

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

ANIMATIONS_GIF = $(patsubst %,screenshot_%.gif,$(ANIMATIONS))

# Detect whether we need a virtual display for screenshot capture
XVFB_RUN := $(shell if [ -z "$$DISPLAY" ] && [ -z "$$WAYLAND_DISPLAY" ] && [ -z "$$WAYLAND_SOCKET" ] && command -v xvfb-run >/dev/null 2>&1; then echo "xvfb-run --auto-servernum --server-args='-screen 0 1280x720x24'"; fi)

# Output directory for newly captured screenshots.  Defaults to screenshots-new/ so that
# running `make screenshots` does not modify the git-tracked screenshots/ directory.
# Override via the environment or on the command line: make screenshots OUT_DIR=screenshots
OUT_DIR ?= screenshots-new

# Arguments: example name, cargo features. The example may save the PNG to the project root
# or directly to $(OUT_DIR)/; either location is accepted.
define capture_screenshot
	@echo "=== Capturing $(1) screenshot ==="
	$(XVFB_RUN) env DSD_SCREENSHOT=1 timeout 120 cargo run --example $(1) --features $(2) 2>&1 || true
	@if [ -f $(OUT_DIR)/screenshot_$(1).png ]; then \
		ffmpeg -y -i $(OUT_DIR)/screenshot_$(1).png -vcodec gif \
			$(OUT_DIR)/screenshot_$(1).gif 2>/dev/null && \
		echo "Saved $(OUT_DIR)/screenshot_$(1).gif" || \
		echo "Warning: ffmpeg GIF encoding failed for $(1)"; \
		rm -f $(OUT_DIR)/screenshot_$(1).png; \
	elif [ -f screenshot_$(1).png ]; then \
		ffmpeg -y -i screenshot_$(1).png -vcodec gif \
			$(OUT_DIR)/screenshot_$(1).gif 2>/dev/null && \
		echo "Saved $(OUT_DIR)/screenshot_$(1).gif" || \
		echo "Warning: ffmpeg GIF encoding failed for $(1)"; \
		rm -f screenshot_$(1).png; \
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
		echo "Encoding frames to GIF..."; \
		ffmpeg -y -framerate 30 -i frames/screenshot_$(1)_frame_%04d.png \
			-vcodec gif -loop 0 \
			$(OUT_DIR)/screenshot_$(1).gif 2>/dev/null && \
		echo "Saved $(OUT_DIR)/screenshot_$(1).gif" || \
		echo "Warning: ffmpeg GIF encoding failed for $(1)"; \
		rm -f frames/screenshot_$(1)_frame_*.png; \
	else \
		echo "Warning: no frames found for $(1)"; \
	fi
endef

# Arguments: example name, output name, cargo features. A long tour is recorded like an animation,
# then encoded at 20 fps and 960 pixels wide so that it stays small enough for the README.
define capture_tour
	@echo "=== Capturing $(2) tour ==="
	@mkdir -p frames
	$(XVFB_RUN) env DSD_SCREENSHOT=1 timeout 1800 cargo run --release --example $(1) --features $(3) 2>&1 || true
	@if ls frames/screenshot_$(2)_frame_*.png 1>/dev/null 2>&1; then \
		echo "Encoding frames to GIF..."; \
		ffmpeg -y -framerate 30 -i frames/screenshot_$(2)_frame_%04d.png \
			-vf "fps=20,scale=960:-1:flags=lanczos" -vcodec gif -loop 0 \
			$(OUT_DIR)/screenshot_$(2).gif 2>/dev/null && \
		echo "Saved $(OUT_DIR)/screenshot_$(2).gif" || \
		echo "Warning: ffmpeg GIF encoding failed for $(2)"; \
		rm -f frames/screenshot_$(2)_frame_*.png; \
	else \
		echo "Warning: no frames found for $(2)"; \
	fi
endef

# Records only the tour of one of the two larger examples into screenshots/.
cozy_castle_tour:
	$(call capture_tour,cozy_castle_3d,cozy_castle,shapes-3d)

kart_racer_editor_tour:
	$(call capture_tour,kart_racer_editor,kart_racer_editor,shapes-3d)

screenshots:
	@echo "=== Building all examples ==="
	cargo build --examples --features shapes-2d,shapes-3d
	@mkdir -p $(OUT_DIR)
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
	$(call capture_animation,constellation,gradmesh)
	@echo ""
	$(call capture_animation,dag,gradmesh)
	@echo ""
	@echo "!!! WARNING: cozy_castle and kart_racer_editor tours are NOT regenerated here."
	@echo "!!! They take a long time and produce large files (~10 MB combined)."
	@echo "!!! Run 'make cozy_castle_tour' or 'make kart_racer_editor_tour' individually when needed."
	@echo ""
	$(call capture_animation,physics_lab,shapes-3d)
	@echo ""
	@echo "=== Screenshots saved to $(OUT_DIR)/ ==="
	@ls -la $(OUT_DIR)/

clean:
	cargo clean

# Release target: sanity-checks all screenshots and packages them into a zip for GitHub Releases.
# Use this locally before uploading, and in CI to gate releases.
VERSION := $(shell grep '^version =' Cargo.toml | head -1 | sed 's/version = "\([^"]*\)".*/\1/')

release-zip: screenshots
	@echo "=== Validating screenshots ==="
	@for f in $(OUT_DIR)/*.gif; do \
		if [ -f "$$f" ]; then \
			if ! file "$$f" | grep -q "GIF"; then \
				echo "ERROR: $$f is not a valid GIF"; exit 1; \
			fi; \
		fi; \
	done
	@count=$$(ls $(OUT_DIR)/*.gif 2>/dev/null | wc -l); \
	echo "Found $$count screenshot files"
	@cd $(OUT_DIR) && zip -r ../screenshots-v$(VERSION).zip *.gif 2>/dev/null || \
		(cd $(OUT_DIR) && zip ../screenshots-v$(VERSION).zip *.gif)
	@echo "Created screenshots-v$(VERSION).zip ($$(du -h screenshots-v$(VERSION).zip | cut -f1))"

release: release-zip
	@echo ""
	@echo "Upload to GitHub with:"
	@echo "  gh release create v$(VERSION) screenshots-v$(VERSION).zip --title 'v$(VERSION)' --notes '...'"
	@echo ""
	@echo "Or amend an existing release:"
	@echo "  gh release upload v$(VERSION) screenshots-v$(VERSION).zip --clobber"
