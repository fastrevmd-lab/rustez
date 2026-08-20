"""Tests for candidate-datastore tracking on the public Python surface.

rustnetconf 0.14 only sends ``<discard-changes/>`` on close when the session
actually dirtied the shared candidate. Two pieces of that contract are exposed
to Python, and both are easy to break by adding a method to the native
extension type while forgetting the hand-written ``Device`` / ``_RpcProxy``
wrappers that ``from rustez import Device`` actually returns.

That is not hypothetical: ``Device.touched_candidate()`` shipped in exactly
that broken state (present on ``PyDevice``, missing on ``Device``) and raised
``AttributeError`` for every caller. These are surface tests — they assert the
public wrappers forward, not that the underlying tracking is correct, which
needs a device and is covered by the gated integration tests.
"""

import pytest

from rustez import Device
from rustez._rustez_native import PyDevice


def test_device_wrapper_exposes_touched_candidate():
    """The PUBLIC Device wrapper must expose touched_candidate, not just PyDevice.

    `from rustez import Device` returns the hand-written wrapper, which forwards
    each method explicitly and has no __getattr__ delegation.
    """
    assert hasattr(Device, "touched_candidate")
    assert callable(Device.touched_candidate)


def test_native_device_exposes_touched_candidate():
    """The native extension type backs the wrapper above."""
    assert hasattr(PyDevice, "touched_candidate")
    assert callable(PyDevice.touched_candidate)


def test_native_device_exposes_rpc_xml_candidate_change():
    """Candidate-modifying raw RPCs need a tracked path on the native type."""
    assert hasattr(PyDevice, "rpc_xml_candidate_change")
    assert callable(PyDevice.rpc_xml_candidate_change)


def test_rpc_proxy_exposes_raw_xml_candidate_change():
    """The rpc proxy must offer the tracked variant alongside raw_xml.

    raw_xml() does not mark the candidate; sending <load-configuration> through
    it leaves the shared candidate dirty and blocks the next session's lock.
    """
    from rustez import _RpcProxy

    assert hasattr(_RpcProxy, "raw_xml_candidate_change")
    assert callable(_RpcProxy.raw_xml_candidate_change)
    # The untracked escape hatch must still exist — this is additive, not a swap.
    assert hasattr(_RpcProxy, "raw_xml")
