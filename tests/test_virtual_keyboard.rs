use pam_linuxcampam::virtual_keyboard::VirtualKeyboard;

#[test]
fn handles_initialization_failure_gracefully() {
    let vkb = VirtualKeyboard::new();
    let init_result = vkb.init();
    if !init_result {
        assert!(!vkb.emit_wakeup());
    } else {
        assert!(vkb.emit_wakeup());
    }
}

#[test]
fn multiple_init_calls_are_safe() {
    let vkb = VirtualKeyboard::new();
    let res1 = vkb.init();
    let res2 = vkb.init();
    assert_eq!(res1, res2);
}
