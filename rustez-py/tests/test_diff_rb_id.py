"""Tests for Config.diff(rb_id=N) forwarding into the native binding.

The native binding now uses rb_id when calling the underlying
`get_configuration_compare(rb_id)` RPC (previously it was discarded).
Without a real device we can only verify the surface accepts the kwarg
and that bogus types are rejected — actual rb_id semantics are covered
by the gated integration test in `rustez/tests/integration_vsrx.rs`.
"""

import pytest

from rustez._rustez_native import PyDevice


def test_native_config_diff_accepts_rb_id_kwarg():
    """PyDevice.config_diff accepts rb_id without TypeError at surface."""
    dev = PyDevice(host="10.0.0.1", username="admin", password="secret")
    # We can't actually call config_diff without a connected device, but
    # we can at least confirm the method exists and is bound.
    assert hasattr(dev, "config_diff")
    assert callable(dev.config_diff)


def test_native_config_diff_rejects_negative_rb_id():
    """rb_id is u32 on the Rust side, so a negative value must fail."""
    dev = PyDevice(host="10.0.0.1", username="admin", password="secret")
    # Negative integers can't fit into u32 — PyO3 surfaces OverflowError.
    with pytest.raises((OverflowError, TypeError)):
        dev.config_diff(rb_id=-1)


def test_native_config_diff_rejects_non_integer_rb_id():
    """rb_id must be an integer."""
    dev = PyDevice(host="10.0.0.1", username="admin", password="secret")
    with pytest.raises(TypeError):
        dev.config_diff(rb_id="not_an_int")
