import unittest

from contract_version import (
    UnsupportedContractVersionError,
    check_contract_version,
    is_supported_contract_version,
)


class ContractVersionGuardTests(unittest.TestCase):
    def test_supported_version_is_accepted(self) -> None:
        self.assertTrue(is_supported_contract_version(7))
        self.assertEqual(check_contract_version(lambda: 7), 7)

    def test_backend_refuses_unsupported_version(self) -> None:
        with self.assertRaises(UnsupportedContractVersionError):
            check_contract_version(lambda: 8)

    def test_backend_refuses_older_supported_range_candidate(self) -> None:
        with self.assertRaises(UnsupportedContractVersionError):
            check_contract_version(lambda: 6)

    def test_backend_refuses_non_integer_version(self) -> None:
        with self.assertRaises(UnsupportedContractVersionError):
            check_contract_version(lambda: "7")


if __name__ == "__main__":
    unittest.main()
