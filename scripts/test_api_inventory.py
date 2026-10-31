"""Offline tests for scripts/api-inventory.py: python3 -m unittest discover -s scripts"""

import importlib.util
import pathlib
import unittest

spec = importlib.util.spec_from_file_location(
    "api_inventory", pathlib.Path(__file__).with_name("api-inventory.py")
)
inv = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inv)


class CallsIn(unittest.TestCase):
    def test_positional_call_with_literal_url(self):
        js = "ajaxC.ajaxCall('getX', {a:1}, '/modules/private/api/apiIndexController.php', 'h');"
        self.assertEqual(inv.calls_in(js), {("/modules/private/api/apiIndexController.php", "getX")})

    def test_object_call_with_module_path(self):
        js = """
        function c(){ var self=this; self.modulePathApi = '/modules/cars/api/';
          AjaxCall({ action: 'getCarModalData', url: self.modulePathApi + 'apiCarsController.php', data:{id:1} });
        }"""
        self.assertEqual(inv.calls_in(js), {("/modules/cars/api/apiCarsController.php", "getCarModalData")})

    def test_table_request_key(self):
        js = "simplePaginator:{ request: 'carsPagePagination', requestUrl: '/modules/cars/api/apiCarsController.php' }"
        self.assertIn(("/modules/cars/api/apiCarsController.php", "carsPagePagination"), inv.calls_in(js))

    def test_core_controllers_and_unresolved_calls(self):
        js = "AjaxCall({action:'getPage', url:'/core/api/dashSpaController.php'});"
        self.assertEqual(inv.calls_in(js), {("/core/api/dashSpaController.php", "getPage")})
        two = "x='/modules/a/api/A.php'; y='/modules/b/api/B.php'; AjaxCall({action:'far'+''});" + " " * 900 + "AjaxCall({action: 'lost'})"
        self.assertIn(("?", "lost"), inv.calls_in(two))


class UsedActions(unittest.TestCase):
    def test_reads_tool_actions_and_direct_rpc_calls(self):
        used = inv.used_actions()
        self.assertIn("getUserHousekeepers", used)  # a tool action
        self.assertIn("getMonoParams", used)  # a direct client.rpc(...) call in pay_link
        self.assertNotIn("payByClientCard", used)


if __name__ == "__main__":
    unittest.main()
