#!/usr/bin/env python3
"""Drive the web build in headless Chromium: load it, start audio, play,
take screenshots, report console errors and what the worklet produces.

    tools/shoot.py http://127.0.0.1:8765/ out_dir [script]

`script` is a comma-separated list of steps run after loading:
  shot:NAME            full-page screenshot
  click:SELECTOR       click the first match
  ctrlclick:SELECTOR   Ctrl-click the first match
  key:KEY              press a key (e.g. Space)
  wait:MS              wait
  eval:JS              evaluate JS and print the result
  drag:SELECTOR@DY     drag the first match vertically by DY pixels (up is negative)
  text:SELECTOR        print the inner text of every match
  eshot:SELECTOR@NAME  screenshot of the first match
"""
import sys
import time

from playwright.sync_api import sync_playwright


def main():
    url, out = sys.argv[1], sys.argv[2]
    script = sys.argv[3] if len(sys.argv) > 3 else "shot:initial"
    with sync_playwright() as p:
        browser = p.chromium.launch(args=["--autoplay-policy=no-user-gesture-required"])
        page = browser.new_page(viewport={"width": 1480, "height": 1000})
        logs = []
        page.on("console", lambda m: logs.append(f"[{m.type}] {m.text}"))
        page.on("pageerror", lambda e: logs.append(f"[pageerror] {e}"))
        page.goto(url)
        page.wait_for_selector(".root", timeout=20000)
        time.sleep(1.0)
        for step in script.split(","):
            kind, _, arg = step.partition(":")
            if kind == "shot":
                page.screenshot(path=f"{out}/{arg}.png", full_page=True)
                print("shot", arg)
            elif kind == "click":
                page.locator(arg).first.click()
            elif kind == "ctrlclick":
                page.locator(arg).first.click(modifiers=["Control"])
            elif kind == "key":
                page.keyboard.press(arg)
            elif kind == "wait":
                time.sleep(int(arg) / 1000)
            elif kind == "drag":
                sel, _, dy = arg.rpartition("@")
                page.locator(sel).first.scroll_into_view_if_needed()
                box = page.locator(sel).first.bounding_box()
                x, y = box["x"] + box["width"] / 2, box["y"] + box["height"] / 2
                page.mouse.move(x, y)
                page.mouse.down()
                page.mouse.move(x, y + float(dy), steps=8)
                page.mouse.up()
            elif kind == "eshot":
                sel, _, name = arg.rpartition("@")
                page.locator(sel).first.scroll_into_view_if_needed()
                page.locator(sel).first.screenshot(path=f"{out}/{name}.png")
                print("shot", name)
            elif kind == "text":
                print("text", arg, "=>", page.locator(arg).all_inner_texts())
            elif kind == "eval":
                print("eval", arg, "=>", page.evaluate(arg))
        for line in logs:
            print(line)
        browser.close()


if __name__ == "__main__":
    main()
