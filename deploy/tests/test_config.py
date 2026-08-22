from __future__ import annotations

import copy

import pytest
from agentx_deploy.config import load_values, selected_targets


def test_all_values_files_are_valid() -> None:
    for name in ("local.yaml", "dockerhub-beta.yaml", "production.example.yaml"):
        config = load_values(f"deploy/values/{name}")
        assert config.values["global"]["environment"] in {"local", "production"}
        assert len(set(config.namespaces.values())) == 3


def test_run_id_scopes_all_physical_namespaces() -> None:
    config = load_values("deploy/values/local.yaml", run_id="unit-123")
    assert config.namespaces == {
        "control": "agentx-e2e-control-unit-123",
        "runtime": "agentx-e2e-runtime-unit-123",
        "dependencies": "agentx-e2e-deps-unit-123",
    }
    assert config.values["global"]["ingress"]["className"] == "agentx-e2e-unit-123"


def test_production_rejects_generated_secrets() -> None:
    config = load_values("deploy/values/production.example.yaml")
    values = copy.deepcopy(config.values)
    values["global"]["secrets"]["mode"] = "generated-local"
    config = config.__class__(config.path, config.root, values)
    from agentx_deploy.config import validate_semantics

    with pytest.raises(ValueError, match="existing-kubernetes"):
        validate_semantics(config)


def test_targets_have_stable_dependency_order() -> None:
    assert selected_targets("all") == ("dependencies", "control", "runtime", "observability")
    assert selected_targets("runtime") == ("runtime",)
