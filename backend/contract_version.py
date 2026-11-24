"""Contract schema-version compatibility helpers for backend tooling."""

from __future__ import annotations

import os
from collections.abc import Callable


SUPPORTED_CONTRACT_SCHEMA_MIN = int(
    os.environ.get("ESCROW_SUPPORTED_SCHEMA_MIN", "7")
)
SUPPORTED_CONTRACT_SCHEMA_MAX = int(
    os.environ.get("ESCROW_SUPPORTED_SCHEMA_MAX", "7")
)


class UnsupportedContractVersionError(RuntimeError):
    """Raised when a deployed escrow contract schema is not supported."""


def is_supported_contract_version(
    version: object,
    minimum: int = SUPPORTED_CONTRACT_SCHEMA_MIN,
    maximum: int = SUPPORTED_CONTRACT_SCHEMA_MAX,
) -> bool:
    return (
        isinstance(version, int)
        and not isinstance(version, bool)
        and minimum <= version <= maximum
    )


def assert_supported_contract_version(
    version: object,
    minimum: int = SUPPORTED_CONTRACT_SCHEMA_MIN,
    maximum: int = SUPPORTED_CONTRACT_SCHEMA_MAX,
) -> int:
    if not is_supported_contract_version(version, minimum, maximum):
        raise UnsupportedContractVersionError(
            f"Unsupported escrow contract schema version {version!r}; "
            f"supported range is {minimum}-{maximum}"
        )
    return version


def check_contract_version(
    get_version: Callable[[], object],
    minimum: int = SUPPORTED_CONTRACT_SCHEMA_MIN,
    maximum: int = SUPPORTED_CONTRACT_SCHEMA_MAX,
) -> int:
    """Read get_version() and refuse unsupported contracts."""
    return assert_supported_contract_version(get_version(), minimum, maximum)
