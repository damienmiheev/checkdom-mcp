# CheckDom unofficial API — resident (private client) surface

Reverse-engineered from the dashboard's own frontend JavaScript (`https://checkdom.in.ua/dash/`)
and checked against a real resident account on 2026-10-31. Unofficial and undocumented: it can
change without notice. `src/tools.rs` is the authoritative list of what this server sends; this
file explains why.

**Legend.** ✅ = called live with a real account and returned data. 📄 = taken from the
dashboard's JavaScript only (write actions are not run live without the owner's say-so).

## Transport

Every call is an RPC over HTTPS:

```
POST https://checkdom.in.ua/modules/<module>/api/<Controller>.php
Content-Type: application/json
Cookie: session=<token>

{"data": {"action": "<actionName>", "langModule": "", ...params}}
```

Response (JSON text, HTTP 200 even on logical errors):

```json
{"code": "000", "message": "Success", "data": { ..., "curVersionTime": 1730000000 }}
```

| code  | meaning                                  |
|-------|------------------------------------------|
| `000` | success                                  |
| `205` | session expired → log in again           |
| other | error, human-readable text in `message` (Ukrainian) |

A non-JSON body or HTTP 500 is a server error (often a missing parameter). Never surface the raw
body to the model. Responses may carry `curVersionTime`, `langData` and `payToken`; this server
drops them (the pay token belongs to the dashboard's own payment widget).

## Login ✅

RPC `userLogin` with `{login, pass}` (CIF or email, password) on
`/modules/front/api/loginController.php`, the same envelope as every other call, no CSRF token.
The `session` cookie in the response's `Set-Cookie` is the token. The HTML form that posts to
`/login` is a dead path: `POST /login` is answered `301 → /login/`, which drops the body.

## Identity and roles ✅

- `role_id` for a resident is `3` (`ROLE_PRIVATE`; the `currentRole` cookie). Other roles: 1 core
  employee, 2 housekeeper, 4 business, 5 super-admin.
- `role_owner_id` for a resident is the user's own id (`userRolesController.getRoleOwnerId`).
  Exceptions: the reporting charts and `getBankAccountsList` take the **housekeeper's** id
  there (and `getBankAccountsList` sends `role_id: 2`).
- The user id is not returned by any profile call; the dashboard page embeds it as
  `curUser.id = '<id>'`. The server reads it from `GET /dash/` once per session token.
- Many actions take the user id as `user_id`, `userId` or `id`; the dashboard sends role ids
  in tables as strings (`"3"`).

## Tables (paginated lists) ✅

The dashboard's tables (`simplePaginator.js`) send exactly:

```json
{"pagination": {"page": 1, "perPage": "25",
                "orderBy": {"param": "id", "dir": "desc"},
                "filters": {"<data-type>": "<value>", "orFilters": {"<data-type>": ["v"]}}},
 "additionalData": {}}
```

- `filters` sits **inside** `pagination`; `perPage` is a string; dates are `YYYY-MM-DD`.
- Each table page also has hidden filter inputs, sent in `pagination.filters` under their
  `data-type`. Send the same parameters as the dashboard page does (`src/tools.rs`).
  Response: `data.result[]` and `data.total`.

| Table | `pagination.filters` | `additionalData` |
|---|---|---|
| `getSupportRequestsPagination` | `role_id: "3"`, `role_owner_id`, `filterByUser`: user id (string) | `{}` |
| `filterUserTemporaryPassPagination` | `role_id: "3"`, `role_owner_id` | `{}` |
| `filterUserLongtermPassPagination` | `role_id: "3"`, `role_owner_id` | `{}` |
| `carsPagePagination` | `role_id: "3"`, `role_owner_id` | `{}` |
| `getVotingsPagination` | `role_id: "3"`, `role_owner_id`, `type: "0"` | `{}` |
| `getHousekeeperFilesPagination` | `role_id: "3"`, `role_owner_id`, `source_id` (0 all, 1 mine, 2 shared with me, 3 to sign) | `role_owner_id`, `role_id: "3"`, `source_id` |
| `getStatementsPagination` | — | `role_owner_id`, `role_id: "3"` |
| `getMessagesPagination` | — | `role_owner_id`, `role_id: "3"`, `folder` (0 inbox, 1 archive) |
| `getIncomingInvoicesPagination`, `getClientPaymentsPagination` | — | `{}` |

## Actions used by the tools

Controller paths are under `/modules/`. Parameters marked *user* are the user id.

### Buildings, property, balance

| | Controller | Action | Params |
|---|---|---|---|
| ✅ | `private/api/apiIndexController.php` | `getUserHousekeepers` | `user_id`: *user* → `housekeepers[{id, short_name}]`, `addresses[]` |
| ✅ | `dash/api/apiProperty.php` | `getUserHouses` | `id`: *user*, `mode` (1 owned, 2 rented), `type: "3"` |
| ✅ | `dash/api/apiProperty.php` | `getApartment` | `id` (property id), `mode` |
| ✅ | `dash/api/apiProperty.php` | `getToPayAmount` | `user_property_apartment_id`, `mode`, `account_id` (`"*"` all) |
| ✅ | `private/api/apiIndexController.php` | `getUserStatsDataByService` | `payfor`, `hk_address_id` (`"*"` all) |
| ✅ | `houseKeeper/api/apiDebtStatusController.php` | `getHousekeeperFilters` | `hk_id` |
| ✅ | `houseKeeper/api/apiDebtStatusController.php` | `getDebtStatus` | `hk_id`, `house_id`, `section`, `floor` — all required, `"*"` = all (omitting them → HTTP 500) |

### Reports (Звітність)

| | Controller | Action | Params |
|---|---|---|---|
| ✅ | `houseKeeper/api/apiServicesController.php` | `getAllActiveServices` | `hk_id` |
| ✅ | `houseKeeper/api/apiHouseController.php` | `getHouses` | `hk_id` |
| ✅ | `houseKeeper/api/apiSubscriberAccountingController.php` | `initChart` (settlements) | `role_id: 3`, `role_owner_id`: hk id, `chart_type` (0 billed vs paid + totals, 1 billing by service, 2 payments by service), `chart_house` (`"*"`, `"-1"`, id), `chart_service` (`"*"`, id), `chart_interval: ["DD.MM.YYYY","DD.MM.YYYY"]` |
| ✅ | `bankRequisites/api/apiRequisitesController.php` | `getBankAccountsList` | `role_id: 2`, `role_owner_id`: hk id |
| ✅ | `houseKeeper/api/apiSubscriberAccountingController.php` | `initChart` (cash flow) | `mode: 1`, `role_id: 3`, `role_owner_id`: hk id, `chart_type` (1 in/out, 2 income by category, 3 expenses by category), `chart_account` (`"*"`, id), `chart_interval` |

### Invoices and payments

| | Controller | Action | Params |
|---|---|---|---|
| ✅ | `private/api/apiInvoicesController.php` | `getIncomingInvoicesPagination` | table |
| ✅ | `invoices/api/apiInvoicesController.php` | `getIncomingInvoiceCard` | `id` |
| ✅ | `private/api/apiClientPaymentsController.php` | `getClientPaymentsPagination` | table |
| ✅ | `private/api/apiClientPaymentsController.php` | `getClientPaymentStats` | `user_id`: *user* |
| ✅ | `private/api/apiAccount.php` | `getUserRequisites` | — (payment codes IDS/IDSP/IDSA) |
| ✅ | `private/api/apiIndexController.php` | `getPaymentProviders` | — → `providers[]`: 1 LiqPay, 5 monobank, 6 Portmone |
| 📄 W | `private/api/apiIndexController.php` | `placeOrder` | `params: [{user_id, account_id (string), hk_id, amount, ref_invoice_id}]` → `order_id` (−1 = failed). Creates a pending order; charges nothing |
| 📄 | `private/api/apiIndexController.php` | `getMonoParams` | `order_id` → `params[0].page_url` (monobank hosted page) |
| 📄 | `private/api/apiIndexController.php` | `getLiqpayWidgetParams` | `order_id` → `form[0].{data, sig}` → `https://www.liqpay.ua/api/3/checkout?data=…&signature=…` |
| ⛔ | `private/api/apiIndexController.php` | `payByClientCard` | charges a saved card — **never exposed** |

### Support requests (`supportRequests/api/apiSupportRequest.php`)

| | Action | Params |
|---|---|---|
| ✅ | `getSupportRequestsPagination` | table (filters above). Status 0 new, 1 in progress, 2 waiting for the client, 3 closed |
| ✅ | `getCommentsByRequestId` | `id` |
| ✅ | `getUserHouseKepers` (sic) | `user_id`: *user* |
| ✅ | `getSupportRequestCategoryByOwnerIdAndUserType` | `id`: hk id, `type: 1` (a private person) |
| ✅ | `getSupportSubCategoryByCategoryId` | `id`: category id (−1 = "Інше") |
| ✅ | `getUserPropertyByUserIdHkId` | `userId`: *user*, `hkId` → addresses grouped by type 0–4; option value `"<address_id>_<address_apartment>"` |
| ✅ | `getSupportSubCategoryScheduleMap` | `sub_category_id`, `from_date`, `to_date` (`YYYY-MM-DD`) |
| 📄 W | `saveSupportRequest` | `id: -1`, `user_id`, `author_user_id`: *user*, `property_id` (string, above), `property_type` (0–4), `type: 1` (housekeeper), `support_id`: hk id, `category`, `sub_category`, `description` (1–999), `files: "[]"`, `status: 0`, `comment_type: 0`, `request_entity_type: 1`, `request_entity_id: null`, `subscriber_id`, `schedule` (`{start_datetime, end_datetime}` or null) |
| 📄 W | `saveComment` | `request_id`, `comment_text`, `comment_files: "[]"`, `user_id`: *user*, `comment_type: 0`, optional `status: 1` (answering a request waiting for the client) |
| 📄 W | `changeStatus` | `id`, `status: 3`, `closed_status: 2` (cancelled by the client). No reopen from the resident UI |

### Messages and news

| | Controller | Action | Params |
|---|---|---|---|
| ✅ | `internalMessages/api/apiInternalMessages.php` | `getMessagesPagination` | table, see above |
| ✅ | `internalMessages/api/apiInternalMessages.php` | `countUnreadMessages` | `role_owner_id`: *user*, `role_id: "3"` (top level) |
| 📄 W | `internalMessages/api/apiInternalMessages.php` | `viewMessage` | `id`: message id, `record_id`: inbox row id. Marks the message read |
| ✅ | `serviceMessages/api/apiServiceMessage.php` | `getServiceMessagesByUserId` | `id`: *user* |
| ✅ | `news/api/apiNews.php` | `getNewsPseudoPagination` | `userId`: *user*, `shown` (offset), optional `searchName`, `searchStartDate`, `searchEndDate` |

### Polls and votes (`voting/api/apiVotingController.php`)

| | Action | Params |
|---|---|---|
| ✅ | `getVotingRequests` | `type: 0`, `role_owner_id`: *user*, `role_id: "3"` → `requests[]` (not `result`) |
| ✅ | `getVotingsPagination` | table (filters above) |
| 📄 | `getVotingCard` | `id`, `voter: true`, `role_owner_id`, `role_id` → `voting{questions[{id, propositions[{id, proposition}]}], votes[]}` |
| 📄 W | `vote` | `question_id`, `proposition_id`, `voting_id`, `role_id`, `role_owner_id` |

### Passes and cars

| | Controller | Action | Params |
|---|---|---|---|
| ✅ | `houseKeeper/api/apiTemporaryPassController.php` | `filterUserTemporaryPassPagination` | table (filters above) |
| ✅ | `houseKeeper/api/apiTemporaryPassController.php` | `getTemporaryPassModalData` | `pass_id`. Share link `https://checkdom.in.ua/pass/<token>` |
| ✅ | `houseKeeper/api/apiTemporaryPassController.php` | `getUserApartments` | — |
| 📄 W | `houseKeeper/api/apiTemporaryPassController.php` | `cancelPass` | `pass_id` |
| ✅ | `houseKeeper/api/apiLongtermPassController.php` | `filterUserLongtermPassPagination` | table (filters above) |
| ✅ | `cars/api/apiCarsController.php` | `carsPagePagination`, `getCarModalData` | table; `id` |

### Documents

| | Controller | Action | Params |
|---|---|---|---|
| ✅ | `files/api/apiFile.php` | `getHousekeeperFilesPagination` | table, see above |
| ✅ | `files/api/apiStatement.php` | `getStatementsPagination` | table, see above |
| 📄 W | `files/api/apiStatement.php` | `getAccountingStatusPDF` | `user_property_apartment_id`, `mode` → `file.id`; stores a new statement |

## Not exposed (deliberately)

- **Money:** `payByClientCard`, saved-card management (`getCard`, `saveAction`, `deleteAction`),
  Portmone card flows, long-term key orders (`savePass` on the long-term controller creates a
  priced order, 200 UAH per key).
- **Irreversible or low value for an assistant:** `deleteCar`, `saveCar`, temporary pass
  creation (`savePass`, many fields), message archive/unarchive, news subscriptions, profile
  edits (`savePersonalInfo`, contacts, Telegram), digital signatures (Дія).
- **Housekeeper/admin only:** subscriber management, imports and exports, initiative groups,
  Jira mapping, mass mailings, `backToAdminAccount`, `sendTestEmail`, disk-space settings.
