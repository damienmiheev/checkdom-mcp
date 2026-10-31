# CheckDom unofficial API — resident (private client) surface

Reverse-engineered from the public frontend JS of `https://checkdom.in.ua/dash/` (2026-10-31).
Unofficial, undocumented, may change without notice.

## Transport

Every call is an RPC over HTTPS:

```
POST https://checkdom.in.ua/modules/<module>/api/<Controller>.php
Content-Type: application/json
Cookie: session=<token>

{"data": {"action": "<actionName>", "langModule": "", ...params}}
```

Response (body is JSON text, HTTP 200 even on logical errors):

```json
{"code": "000", "message": "Success", "data": { ..., "curVersionTime": 1730000000 }}
```

| code  | meaning                                  |
|-------|------------------------------------------|
| `000` | success                                  |
| `205` | session expired → re-login               |
| other | error, human-readable text in `message` (Ukrainian) |

Non-JSON body or HTTP 500 = server error. Treat as failure, never surface raw body to the model.

### Paginated lists

Actions ending in `…Pagination` take:

```json
{"pagination": {"page": 1, "perPage": 25, "orderBy": {"param": "id", "dir": "desc"}},
 "filters": {"<column>": "<value>", "orFilters": {"<column>": ["a", "b"]}}}
```

and return `data.result[]` + `data.total`.

### Auth

- Login: `POST /login` (form-urlencoded) with `_csrf-landing` (scraped from `GET /`),
  `LoginForm[username]` (CIF or email), `LoginForm[password]`.
- Sets cookies: `session` (the token), `name`, `surname`, `roles`, `currentRole`.
- `currentRole` / `roles` identify the active role (resident vs housekeeper); many calls
  take `role_id` + `role_owner_id` explicitly.
- Secondary Laravel backend: `https://be.checkdom.in.ua/api/<path>` with header
  `ApiToken: <session>`. No callers found in the resident dashboard yet.

## Resident actions (MCP candidates)

`R` = read-only, `W` = mutates state, `$` = money.

### Profile & property — `/modules/private/api/…`, `/modules/dash/api/apiProperty.php`

| kind | controller | action | params |
|---|---|---|---|
| R | `private/apiAccount.php` | `initPersonPage` | – |
| R | `private/apiAccount.php` | `getUserRequisites` | – |
| R | `private/apiIndexController.php` | `getUserHousekeepers` | – → `housekeepers[{id, short_name}]`, `addresses[]` |
| R | `dash/apiProperty.php` | `getUserHouses` | – |
| R | `dash/apiProperty.php` | `getApartment` | `id` |
| R | `dash/apiProperty.php` | `getToPayAmount` | `user_property_apartment_id`, `mode`, `account_id` |

### Finance — balance, charges, invoices, payments

| kind | controller | action | params |
|---|---|---|---|
| R | `private/apiIndexController.php` | `getUserStatsDataByService` | `payfor`, `hk_address_id` |
| R | `houseKeeper/apiSubscriberAccountingController.php` | `initChart` | `role_id`, `role_owner_id`, `chart_house`, `chart_type`, `chart_service`, `chart_interval` |
| R | `houseKeeper/apiDebtStatusController.php` | `getDebtStatus` | `hk_id`, `house_id`, `section`, `floor` |
| R | `houseKeeper/apiServicesController.php` | `getAllActiveServices` | – |
| R | `private/apiInvoicesController.php` | `initIncomingPage` | – |
| R | `private/apiInvoicesController.php` | `getIncomingInvoicesPagination` | pagination |
| R | `invoices/apiInvoicesController.php` | `getIncomingInvoiceCard` | `hk_id`, `id` |
| R | `private/apiClientPaymentsController.php` | `getClientPaymentsPagination` | pagination |
| R | `private/apiClientPaymentsController.php` | `getClientPaymentStats` | – |
| R | `bankRequisites/apiRequisitesController.php` | `getBankAccountsList` | – |
| R | `files/apiStatement.php` | `getAccountingStatusPDF` | (statement / довідка PDF) |
| $ | `private/apiIndexController.php` | `getPaymentProviders`, `placeOrder`, `getLiqpayWidgetParams`, `getMonoParams` | `order_id` |
| $ | `private/apiIndexController.php` | `payByClientCard` | **never expose** (charges a saved card) |

### Support requests (заявки) — `/modules/supportRequests/api/apiSupportRequest.php`

| kind | action | params |
|---|---|---|
| R | `getSupportRequestsPagination` | pagination → `result[{id, date_time, status{…}, category_id, sub_category_id, sla_datetime, jira_issue, …}]` |
| R | `getSupportRequestsStats` | – |
| R | `getCommentsByRequestId` | `id` |
| R | `getUserHouseKepers` | – |
| R | `getUserPropertyByUserIdHkId` | `userId`, `hkId` |
| R | `getSupportRequestCategoryByOwnerIdAndUserType` | `id` (hk id), `type` (entity type, 1 = individual) |
| R | `getSupportSubCategoryByCategoryId` | `id` |
| R | `getSupportSubCategoryScheduleMap` | (time slots for sub-categories with `schedule_settings`) |
| W | `saveSupportRequest` | `user_id`, `author_user_id`, `property_id`, `property_type`, `type`, `support_id` (hk id), `category`, `sub_category`, `description` (1–999 chars), `files` (JSON array of file ids), `status`, `user_full_name`, `comment_type: 0`, `request_entity_type`, `request_entity_id`, `subscriber_id`, `schedule` (`{start_datetime, end_datetime}` or `null`) |
| W | `saveComment` | `comment_text`, `comment_files` (JSON array), `user_id`, `request_id`, `comment_type: 0`, optional `status: 1` (reopen) |
| W | `changeStatus` | (close / reopen) |

Category/sub-category id `-1` = "Інше" (other), added client-side.

### Messages & news

| kind | controller | action | params |
|---|---|---|---|
| R | `internalMessages/apiInternalMessages.php` | `getMessagesPagination` | pagination + `role_owner_id`, `role_id` |
| R | `internalMessages/apiInternalMessages.php` | `countUnreadMessages` | `role_owner_id`, `role_id` (**required**) |
| R/W | `internalMessages/apiInternalMessages.php` | `viewMessage` | `id`, `record_id` (marks read) |
| W | `internalMessages/apiInternalMessages.php` | `archiveMessages`, `unarchiveMessages` | ids |
| R | `serviceMessages/apiServiceMessage.php` | `getServiceMessagesByUserId` | – |
| R | `news/apiNews.php` | `getNewsPseudoPagination` | pagination |

### Voting (голосування)

| kind | controller | action | params |
|---|---|---|---|
| R | `voting/apiVotingController.php` | `getVotingsPagination` | pagination + `role_id`, `role_owner_id`, type filter |
| R | `voting/apiVotingController.php` | `getVotingCard` | `id` |
| W | `voting/apiVotingController.php` | `vote` | `question_id`, `proposition_id`, `voting_id`, `role_id`, `role_owner_id` |

### Passes (перепустки) & cars

| kind | controller | action | params |
|---|---|---|---|
| R | `houseKeeper/apiTemporaryPassController.php` | `filterUserTemporaryPassPagination` | pagination + `role_id`, `role_owner_id` |
| R | `houseKeeper/apiTemporaryPassController.php` | `getTemporaryPassModalData`, `getUserApartments` | – |
| W | `houseKeeper/apiTemporaryPassController.php` | `savePass`, `cancelPass` | pass fields |
| R | `houseKeeper/apiLongtermPassController.php` | `filterUserLongtermPassPagination`, `getLongtermPassModalData` | – |
| W/$ | `houseKeeper/apiLongtermPassController.php` | `savePass` → `getLiqpayWidgetParams(pass_id)` | paid pass |
| R | `cars/apiCarsController.php` | `carsPagePagination`, `getCarModalData` | – |
| W | `cars/apiCarsController.php` | `saveCar`, `deleteCar` | car fields |

### Documents

| kind | controller | action | params |
|---|---|---|---|
| R | `files/apiStatement.php` | `getStatementsPagination`, `getUserProperty` | – |
| R | `files/apiFile.php` | `getHousekeeperFilesPagination`, `getFileById`, `getFile` | – |

## Out of scope

Housekeeper/admin-only actions (subscriber management, imports, initiative groups,
mass SMS/email, Jira mapping, `backToAdminAccount`, `sendTestEmail`) — not exposed.

## TODO before implementation

- Confirm login flow end-to-end (CSRF, possible 2FA/SMS step, session lifetime).
- Capture exact param sets for `getUserStatsDataByService`, `getSupportSubCategoryScheduleMap`,
  temporary `savePass`.
- File upload for request attachments (`files/apiFile.php` → `uploadFile`).
