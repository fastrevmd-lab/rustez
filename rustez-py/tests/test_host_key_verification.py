"""Tests for SSH host-key verification surface on rustez.Device.

These tests do not require a reachable device — they verify the Python
API accepts the new kwarg and forwards it to the native binding.
"""

import pytest

from rustez import Device
from rustez._rustez_native import PyDevice


def test_native_pydevice_accepts_host_key_fingerprint():
    """The PyO3 PyDevice constructor accepts host_key_fingerprint kwarg."""
    dev = PyDevice(
        host="10.0.0.1",
        username="admin",
        password="secret",
        host_key_fingerprint="SHA256:abc123",
    )
    assert dev is not None


def test_native_pydevice_host_key_fingerprint_optional():
    """Backward compat: omitting the kwarg works as before."""
    dev = PyDevice(host="10.0.0.1", username="admin", password="secret")
    assert dev is not None


def test_native_pydevice_host_key_fingerprint_rejects_non_string():
    """PyO3 surfaces a TypeError when fingerprint is not a string."""
    with pytest.raises(TypeError):
        PyDevice(
            host="10.0.0.1",
            username="admin",
            password="secret",
            host_key_fingerprint=12345,
        )


def test_device_wrapper_forwards_host_key_fingerprint():
    """Device.__init__ forwards host_key_fingerprint to the native binding.

    Verified indirectly: if Device silently swallowed the kwarg via **kwargs,
    a bogus integer would be accepted. We assert it surfaces the underlying
    TypeError from the native call.
    """
    with pytest.raises(TypeError):
        Device(
            host="10.0.0.1",
            user="admin",
            passwd="secret",
            host_key_fingerprint=12345,
        )
