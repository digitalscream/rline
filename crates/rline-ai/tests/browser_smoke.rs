//! Smoke test: attempt a real headless Chromium launch.
//!
//! Run with `cargo test -p rline-ai --test browser_smoke -- --nocapture --ignored`.

use rline_ai::ai_runtime;
use rline_ai::browser::BrowserSession;

#[test]
#[ignore = "requires Chrome/Chromium on PATH"]
fn smoke_launch_screenshot_close() {
    let rt = ai_runtime();
    rt.block_on(async {
        println!("launching...");
        let session =
            match BrowserSession::launch("data:text/html,<h1>hello</h1>", (900, 600)).await {
                Ok(s) => s,
                Err(e) => panic!("launch failed: {e}"),
            };
        println!("launched.");
        let url = session.current_url().await;
        println!("url: {url}");
        let png = session.screenshot().await.expect("screenshot");
        println!("screenshot bytes: {}", png.len());
        assert!(png.len() > 100);
        session.close().await.expect("close");
        println!("closed.");
    });
}

#[test]
#[ignore = "requires Chrome/Chromium on PATH"]
fn smoke_scroll_boundary_detection() {
    let rt = ai_runtime();
    rt.block_on(async {
        // Tall page: 3000px of content, viewport 600px tall.
        let html = "data:text/html,<!doctype html><html><body style='margin:0'>\
                    <div style='height:3000px;background:linear-gradient(red,blue)'></div>\
                    </body></html>";
        let session = BrowserSession::launch(html, (900, 600))
            .await
            .expect("launch");

        let first = session.scroll(600).await.expect("scroll 1");
        println!("scroll 1: before={} after={} max={}", first.before_y, first.after_y, first.max_y);
        assert!(first.moved(), "first scroll should move");
        assert!(!first.at_bottom(), "first scroll should not be at bottom yet");

        // Scroll to bottom in big steps.
        for i in 2..=8 {
            let s = session.scroll(600).await.expect("scroll n");
            println!(
                "scroll {i}: before={} after={} max={} at_bottom={} moved={}",
                s.before_y, s.after_y, s.max_y, s.at_bottom(), s.moved()
            );
            if s.at_bottom() {
                break;
            }
        }

        // One more scroll past the bottom should report no movement.
        let stuck = session.scroll(600).await.expect("scroll stuck");
        println!("stuck: before={} after={} moved={}", stuck.before_y, stuck.after_y, stuck.moved());
        assert!(!stuck.moved(), "scrolling past bottom must not move the page");
        assert!(stuck.at_bottom(), "should report at_bottom");

        session.close().await.expect("close");
    });
}

#[test]
#[ignore = "requires Chrome/Chromium on PATH + network"]
fn smoke_launch_github() {
    let rt = ai_runtime();
    rt.block_on(async {
        println!("launching github...");
        let session = match BrowserSession::launch(
            "https://github.com/ggml-org/llama.cpp/releases",
            (900, 600),
        )
        .await
        {
            Ok(s) => s,
            Err(e) => panic!("launch failed: {e}"),
        };
        let url = session.current_url().await;
        println!("url: {url}");
        let png = session.screenshot().await.expect("screenshot");
        println!("screenshot bytes: {}", png.len());
        session.close().await.expect("close");
    });
}
