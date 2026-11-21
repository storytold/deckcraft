/// "Mark as decorative" starts checked when every selected shape is already decorative.
pub(super) fn alt_text_starts_decorative(decorative: impl IntoIterator<Item = bool>) -> bool {
    let mut any = false;
    for on in decorative {
        if !on {
            return false;
        }
        any = true;
    }
    any
}

#[cfg(test)]
mod alt_text_tests {
    use super::alt_text_starts_decorative;

    #[test]
    fn decorative_starts_checked_only_when_every_shape_is() {
        assert!(!alt_text_starts_decorative([]));
        assert!(!alt_text_starts_decorative([false]));
        assert!(alt_text_starts_decorative([true]));
        assert!(alt_text_starts_decorative([true, true]));
        assert!(!alt_text_starts_decorative([true, false]));
    }
}
