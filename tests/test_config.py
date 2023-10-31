import repotracer.lib.config
from repotracer.lib.config import GlobalConfig, RepoConfig
import dacite


def mock_read_config(mocker, config):
    mocker.patch("repotracer.lib.config.read_config_file", return_value=config)


def mock_dict_config(mocker, config_dict):
    config = dacite.from_dict(GlobalConfig, config_dict)
    mock_read_config(mocker, config)


def test_patch_path(mocker):
    mock_dict_config(mocker, {"repos": {}})
    assert len(repotracer.lib.config.read_config_file().repos) == 0


def test_get_repo_config(mocker):
    mock_dict_config(mocker, {"repos": {"test": {"source": "."}}})
    assert repotracer.lib.config.list_repos() == ["test"]


# def test_get_repo_storage_path(mocker):
#     cases = {
#         "repos": {
#             "test_empty": {"name": "test_empty", "source": "..."},
#             "test_path": {
#                 "name": "test_path",
#                 "source": "...",
#                 "storage_path": "/tmp/test",
#             },
#         }
#     }

#     mock_dict_config(mocker, cases)
#     assert repotracer.lib.config.get_repo_storage_path("test_empty") == "test_empty"
#     assert repotracer.lib.config.get_repo_storage_path("test_path") == "/tmp/test"
