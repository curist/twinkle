# Advent of Code in Twinkle.
#
# All recipes run with this directory as the working directory, so the Twinkle
# file paths inside solutions (e.g. "y2024/day01/input.txt") resolve correctly.
#
#   make list                   list scaffolded days, grouped by year
#   make new YEAR=2024 DAY=1     scaffold a fresh day from the template
#   make pull YEAR=2024 DAY=1    scaffold if needed, then fetch input + statement
#   make solve YEAR=2024 DAY=1   run one day, printing answers on the real input
#   make test                   run every year's tests
#   make test YEAR=2024         run one year's tests
#   make gen                    regenerate the suite aggregators from disk

TWK ?= ../../target/twk

# Zero-padded day, e.g. DAY=1 -> DD=01. Empty when DAY is unset.
DD := $(if $(DAY),$(shell printf '%02d' $(DAY)),)
DIR := y$(YEAR)/day$(DD)

# AoC's polite-use guidance asks for an identifying User-Agent.
UA := github.com/curist/twinkle aoc example

.PHONY: list new pull solve test gen help

help:
	@sed -n 's/^#   //p' Makefile

# --- listing --------------------------------------------------------------

list:
	@found=0; \
	for ydir in y*/; do \
		[ -d "$$ydir" ] || continue; \
		year=$${ydir%/}; \
		echo "$${year#y}:"; \
		for ddir in "$$ydir"day*/; do \
			[ -f "$$ddir/solution.tw" ] || continue; \
			found=1; \
			day=$$(basename "$$ddir"); \
			if [ -f "$$ddir/input.txt" ]; then tag="input pulled"; else tag="no input"; fi; \
			printf '  %s  (%s)\n' "$$day" "$$tag"; \
		done; \
	done; \
	[ "$$found" -eq 0 ] && echo "no days yet — scaffold one with: make new YEAR=2024 DAY=1" || true

# --- scaffolding ----------------------------------------------------------

new: require-day
	@if [ -d "$(DIR)" ]; then echo "$(DIR) already exists"; exit 1; fi
	@mkdir -p "$(DIR)"
	@sed -e 's/__YEAR__/$(YEAR)/g' -e 's/__DAY__/$(DAY)/g' -e 's/__DD__/$(DD)/g' \
		template/solution.tw.tmpl > "$(DIR)/solution.tw"
	@sed -e 's/__YEAR__/$(YEAR)/g' -e 's/__DAY__/$(DAY)/g' -e 's/__DD__/$(DD)/g' \
		template/main.tw.tmpl > "$(DIR)/main.tw"
	@: > "$(DIR)/sample.txt"
	@$(MAKE) --no-print-directory gen
	@echo "scaffolded $(DIR) — paste the example into $(DIR)/sample.txt and start solving"

# --- fetching -------------------------------------------------------------

pull: require-day
	@if [ ! -d "$(DIR)" ]; then $(MAKE) --no-print-directory new YEAR=$(YEAR) DAY=$(DAY); fi
	@SESSION="$${AOC_SESSION:-$$(cat .session 2>/dev/null || true)}"; \
	if [ -z "$$SESSION" ]; then \
		echo "no session cookie — set AOC_SESSION or create a .session file"; exit 1; \
	fi; \
	echo "fetching input -> $(DIR)/input.txt"; \
	if err=$$(curl -fsS -A "$(UA)" -b "session=$$SESSION" \
		"https://adventofcode.com/$(YEAR)/day/$(DAY)/input" -o "$(DIR)/input.txt.tmp" 2>&1); then \
		mv "$(DIR)/input.txt.tmp" "$(DIR)/input.txt"; \
	else \
		rm -f "$(DIR)/input.txt.tmp"; \
		echo "input fetch failed ($$err) — check the cookie, or the puzzle may not be unlocked yet" >&2; \
		exit 1; \
	fi; \
	echo "fetching statement -> $(DIR)/statement.md"; \
	if err=$$(curl -fsS -A "$(UA)" -b "session=$$SESSION" \
		"https://adventofcode.com/$(YEAR)/day/$(DAY)" -o "$(DIR)/statement.html.tmp" 2>&1); then \
		mv "$(DIR)/statement.html.tmp" "$(DIR)/statement.html"; \
		perl -0777 -ne 'while(/<article.*?>(.*?)<\/article>/sg){$$a=$$1;$$a=~s/<[^>]+>//g;$$a=~s/&gt;/>/g;$$a=~s/&lt;/</g;$$a=~s/&amp;/&/g;print$$a,"\n\n";}' \
			"$(DIR)/statement.html" > "$(DIR)/statement.md" || true; \
	else \
		rm -f "$(DIR)/statement.html.tmp"; \
		echo "statement fetch failed ($$err) — continuing, input is what matters" >&2; \
	fi

# --- running --------------------------------------------------------------

solve: require-day
	@$(TWK) run "$(DIR)/main.tw"

test: gen
ifeq ($(strip $(YEAR)),)
	@$(TWK) run main.tw
else
	@$(TWK) run "y$(YEAR)/main.tw"
endif

gen:
	@sh bin/gen.sh
	@$(TWK) fmt main.tw y*/suite.tw y*/main.tw >/dev/null 2>&1 || true

# --- guards ---------------------------------------------------------------

require-day:
	@if [ -z "$(YEAR)" ] || [ -z "$(DAY)" ]; then \
		echo "usage: make $(MAKECMDGOALS) YEAR=<year> DAY=<day>"; exit 1; \
	fi
