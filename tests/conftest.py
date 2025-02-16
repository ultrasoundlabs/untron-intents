import pytest
from script.deploy_resolver import deploy

@pytest.fixture
def untron_resolver_contract():
    return deploy()