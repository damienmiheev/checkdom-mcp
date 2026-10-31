//! MCP tool definitions and dispatch.
//!
//! Every tool is one row of [`TOOLS`]: a fixed controller path and action,
//! whether it reads or writes, its typed arguments and the constant fields the
//! dashboard always sends. The tool list, the input schemas, the write gate and
//! argument validation are all derived from that table, so they cannot drift
//! apart. Read tools are always available; write tools are only listed and
//! callable with `--allow-writes`. Parameter shapes follow the dashboard's own
//! JavaScript (see docs/API.md).

use crate::client::CheckDomClient;
use crate::error::{CheckDomError, Result};
use serde_json::{Map, Value, json};

// RPC controller paths (see docs/API.md).
const IDX: &str = "/modules/private/api/apiIndexController.php";
const DEBT: &str = "/modules/houseKeeper/api/apiDebtStatusController.php";
const INV: &str = "/modules/private/api/apiInvoicesController.php";
const INV_CARD: &str = "/modules/invoices/api/apiInvoicesController.php";
const PAYMENTS: &str = "/modules/private/api/apiClientPaymentsController.php";
const SUP: &str = "/modules/supportRequests/api/apiSupportRequest.php";
const MSG: &str = "/modules/internalMessages/api/apiInternalMessages.php";
const SERVICE_MSG: &str = "/modules/serviceMessages/api/apiServiceMessage.php";
const NEWS: &str = "/modules/news/api/apiNews.php";
const VOTE: &str = "/modules/voting/api/apiVotingController.php";
const TEMP_PASS: &str = "/modules/houseKeeper/api/apiTemporaryPassController.php";
const KEYS: &str = "/modules/houseKeeper/api/apiLongtermPassController.php";
const CARS: &str = "/modules/cars/api/apiCarsController.php";
const PROP: &str = "/modules/dash/api/apiProperty.php";
const SERVICES: &str = "/modules/houseKeeper/api/apiServicesController.php";
const HOUSES: &str = "/modules/houseKeeper/api/apiHouseController.php";
const CHARTS: &str = "/modules/houseKeeper/api/apiSubscriberAccountingController.php";
const BANK: &str = "/modules/bankRequisites/api/apiRequisitesController.php";
const ACCOUNT: &str = "/modules/private/api/apiAccount.php";
const FILES: &str = "/modules/files/api/apiFile.php";
const STATEMENTS: &str = "/modules/files/api/apiStatement.php";

/// LiqPay's hosted checkout page; the dashboard embeds the same `data` + `signature`.
const LIQPAY_CHECKOUT: &str = "https://www.liqpay.ua/api/3/checkout";
/// The most a single pay link may ask for, as a guard against a mistyped amount.
const MAX_PAY_AMOUNT: f64 = 1_000_000.0;

/// Largest page a list tool asks for, so one call can't pull an unbounded result.
const MAX_PER_PAGE: i64 = 100;
const DEFAULT_PER_PAGE: i64 = 25;
/// The resident ("private client") role; the dashboard sends it as a string.
const ROLE_PRIVATE: &str = "3";

/// Whether a tool only reads, or changes state on CheckDom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Read,
    Write,
}

/// The type of one tool argument, checked before any request is made.
#[derive(Clone, Copy, Debug)]
enum Ty {
    /// A JSON integer (ids, enum codes). `-1` is a valid category id ("Інше").
    Int,
    /// A non-blank string of at most `max` characters.
    Str { max: usize },
    /// A JSON boolean.
    Bool,
    /// A calendar date, `YYYY-MM-DD`.
    Date,
    /// `{"start_datetime": str, "end_datetime": str}` or `null`.
    TimeSlot,
    /// A `YYYY-MM-DD` date sent in the charts' `DD.MM.YYYY` format and appended,
    /// in argument order, to the array under its key (`chart_interval`).
    ChartDate,
    /// An amount of money in UAH: positive, at most 2 decimals.
    Amount,
    /// One of a fixed set of strings.
    OneOf(&'static [&'static str]),
}

/// A value the server fills in rather than the model.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Val {
    Int(i64),
    Str(&'static str),
    Bool(bool),
    Null,
    /// The logged-in user's id (resolved from the session).
    UserId,
    /// The same id as a string, as the dashboard's table filters send it.
    UserIdStr,
}

/// Where a field goes in the request body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// Next to `action` in `data`.
    Top,
    /// In `data.additionalData`, which paginated tables send next to `pagination`.
    Additional,
    /// In `data.pagination.filters`: the hidden filters a dashboard table sends.
    Filter,
}

#[derive(Debug)]
struct Arg {
    /// The tool argument's name.
    name: &'static str,
    /// The request field it is sent as.
    key: &'static str,
    ty: Ty,
    required: bool,
    /// Sent when the argument is omitted.
    default: Option<Val>,
    description: &'static str,
}

const fn req(name: &'static str, ty: Ty, description: &'static str) -> Arg {
    Arg {
        name,
        key: name,
        ty,
        required: true,
        default: None,
        description,
    }
}

const fn opt(name: &'static str, ty: Ty, description: &'static str) -> Arg {
    Arg {
        name,
        key: name,
        ty,
        required: false,
        default: None,
        description,
    }
}

impl Arg {
    /// Send the argument under a different request field name.
    const fn sent_as(mut self, key: &'static str) -> Self {
        self.key = key;
        self
    }

    /// Send `value` when the argument is omitted.
    const fn or(mut self, value: Val) -> Self {
        self.default = Some(value);
        self
    }
}

/// A constant field the dashboard always sends with an action.
#[derive(Debug, Clone, Copy)]
struct Fixed {
    key: &'static str,
    val: Val,
    place: Place,
}

const fn top(key: &'static str, val: Val) -> Fixed {
    Fixed {
        key,
        val,
        place: Place::Top,
    }
}

const fn additional(key: &'static str, val: Val) -> Fixed {
    Fixed {
        key,
        val,
        place: Place::Additional,
    }
}

const fn filter(key: &'static str, val: Val) -> Fixed {
    Fixed {
        key,
        val,
        place: Place::Filter,
    }
}

// The hidden filters the dashboard sends with its tables to limit a list to
// your own rows. Always send them, exactly as the dashboard does.
const MY_ROLE: Fixed = filter("role_id", Val::Str(ROLE_PRIVATE));
const MY_ID: Fixed = filter("role_owner_id", Val::UserIdStr);

/// How the request body is built from the validated arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// The arguments as they are.
    Plain,
    /// A dashboard table: `pagination` (with `filters` inside) and
    /// `additionalData`, built from `page` / `per_page`.
    Paginated,
    /// The two-step pay-link flow (see [`pay_link`]).
    PayLink,
}

#[derive(Debug)]
struct Tool {
    name: &'static str,
    description: &'static str,
    access: Access,
    path: &'static str,
    action: &'static str,
    shape: Shape,
    args: &'static [Arg],
    fixed: &'static [Fixed],
    /// For tables whose rows name their owner: a JSON pointer, within each row,
    /// to the owner's user id. Rows owned by anyone else are dropped.
    owner: Option<&'static str>,
}

const HK_ID: &str = "Housekeeper (building manager) id from list_housekeepers.";

/// Every tool this server offers. Adding a tool means adding a row here.
const TOOLS: &[Tool] = &[
    // --- Buildings and balances -------------------------------------------
    Tool {
        name: "list_housekeepers",
        description: "List your building managers (домоуправителі / ОСББ) and their ids. Many \
                      other tools take an `hk_id` from here.",
        access: Access::Read,
        path: IDX,
        action: "getUserHousekeepers",
        shape: Shape::Plain,
        args: &[],
        fixed: &[top("user_id", Val::UserId)],
        owner: None,
    },
    Tool {
        name: "get_debt_filters",
        description: "List the houses, sections and floors of a building, for filtering \
                      get_debt_status.",
        access: Access::Read,
        path: DEBT,
        action: "getHousekeeperFilters",
        shape: Shape::Plain,
        args: &[req("hk_id", Ty::Int, HK_ID)],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "get_debt_status",
        description: "Get the debt radar (платіжний радар) of a building: balances by \
                      apartment. Filter by house, section and floor (from get_debt_filters), or \
                      leave them out for all.",
        access: Access::Read,
        path: DEBT,
        action: "getDebtStatus",
        shape: Shape::Plain,
        args: &[
            req("hk_id", Ty::Int, HK_ID),
            opt(
                "house_id",
                Ty::Str { max: 32 },
                "A house id, or \"*\" for all (default).",
            )
            .or(Val::Str("*")),
            opt(
                "section",
                Ty::Str { max: 32 },
                "A section number, or \"*\" for all (default).",
            )
            .or(Val::Str("*")),
            opt(
                "floor",
                Ty::Str { max: 32 },
                "A floor number, or \"*\" for all (default).",
            )
            .or(Val::Str("*")),
        ],
        fixed: &[],
        owner: None,
    },
    // --- Invoices and payments --------------------------------------------
    Tool {
        name: "list_invoices",
        description: "List incoming invoices (рахунки), newest first.",
        access: Access::Read,
        path: INV,
        action: "getIncomingInvoicesPagination",
        shape: Shape::Paginated,
        args: &[],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "get_invoice",
        description: "Get one incoming invoice with its lines.",
        access: Access::Read,
        path: INV_CARD,
        action: "getIncomingInvoiceCard",
        shape: Shape::Plain,
        args: &[req("id", Ty::Int, "Invoice id from list_invoices.")],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "list_payments",
        description: "List your payments, newest first.",
        access: Access::Read,
        path: PAYMENTS,
        action: "getClientPaymentsPagination",
        shape: Shape::Paginated,
        args: &[],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "get_payment_stats",
        description: "Get totals of your payments.",
        access: Access::Read,
        path: PAYMENTS,
        action: "getClientPaymentStats",
        shape: Shape::Plain,
        args: &[],
        fixed: &[top("user_id", Val::UserId)],
        owner: None,
    },
    // --- Property, balance and reports --------------------------------------
    Tool {
        name: "list_properties",
        description: "List your properties (apartments, parking places) with their building \
                      manager. `mode`: 1 owned (default), 2 rented. A row's `id` is the \
                      `property_id` of get_property and get_amount_to_pay.",
        access: Access::Read,
        path: PROP,
        action: "getUserHouses",
        shape: Shape::Plain,
        args: &[opt("mode", Ty::Int, "1 owned (default), 2 rented.").or(Val::Int(1))],
        fixed: &[top("id", Val::UserId), top("type", Val::Str(ROLE_PRIVATE))],
        owner: None,
    },
    Tool {
        name: "get_property",
        description: "Get one property: areas, owners and renters with their shares, personal \
                      accounts (`accounts[].id`, used by get_pay_link) and the building manager.",
        access: Access::Read,
        path: PROP,
        action: "getApartment",
        shape: Shape::Plain,
        args: &[
            req(
                "property_id",
                Ty::Int,
                "Property `id` from list_properties.",
            )
            .sent_as("id"),
            opt("mode", Ty::Int, "1 owned (default), 2 rented.").or(Val::Int(1)),
        ],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "get_amount_to_pay",
        description: "Get what is due for one property: `balance` (>= 0 to pay, < 0 prepaid) \
                      and the tariffs behind it.",
        access: Access::Read,
        path: PROP,
        action: "getToPayAmount",
        shape: Shape::Plain,
        args: &[
            req(
                "property_id",
                Ty::Int,
                "Property `id` from list_properties.",
            )
            .sent_as("user_property_apartment_id"),
            opt("mode", Ty::Int, "1 owned (default), 2 rented.").or(Val::Int(1)),
            opt(
                "account_id",
                Ty::Str { max: 32 },
                "A personal account id, or \"*\" for all (default).",
            )
            .or(Val::Str("*")),
        ],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "get_balance",
        description: "Get your balance summary from the home page: to pay, debt, fines, \
                      overpayment, per-account amounts and your last payments.",
        access: Access::Read,
        path: IDX,
        action: "getUserStatsDataByService",
        shape: Shape::Plain,
        args: &[
            opt(
                "hk_address_id",
                Ty::Str { max: 32 },
                "One address (`hk_address_id` from \
                list_properties), or \"*\" for all (default).",
            )
            .or(Val::Str("*")),
            opt("payfor", Ty::Int, "1 owned property (default), 2 rented.").or(Val::Int(1)),
        ],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "list_payment_codes",
        description: "List your payment codes (реквізити IDS/IDSP/IDSA) for paying through \
                      other banks, per property and service.",
        access: Access::Read,
        path: ACCOUNT,
        action: "getUserRequisites",
        shape: Shape::Plain,
        args: &[],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "list_payment_providers",
        description: "List the online payment providers available to you (1 LiqPay, 5 \
                      monobank, 6 Portmone).",
        access: Access::Read,
        path: IDX,
        action: "getPaymentProviders",
        shape: Shape::Plain,
        args: &[],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "list_services",
        description: "List the active services (utilities, fees) a building manager bills.",
        access: Access::Read,
        path: SERVICES,
        action: "getAllActiveServices",
        shape: Shape::Plain,
        args: &[req("hk_id", Ty::Int, HK_ID)],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "list_buildings",
        description: "List a building manager's houses (for the `house` filter of \
                      get_settlements_chart).",
        access: Access::Read,
        path: HOUSES,
        action: "getHouses",
        shape: Shape::Plain,
        args: &[req("hk_id", Ty::Int, HK_ID)],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "get_settlements_chart",
        description: "Get the reporting centre's settlements chart (Звітність) by month. \
                      chart_type 0: billed vs paid, with totals (billed, paid, period debt, debt, \
                      overpayment); 1: billing by service; 2: payments by service.",
        access: Access::Read,
        path: CHARTS,
        action: "initChart",
        shape: Shape::Plain,
        args: &[
            req("hk_id", Ty::Int, HK_ID).sent_as("role_owner_id"),
            req(
                "chart_type",
                Ty::Int,
                "0 billed vs paid, 1 billing by service, 2 payments by service.",
            ),
            req("from_date", Ty::ChartDate, "First day, YYYY-MM-DD.").sent_as("chart_interval"),
            req("to_date", Ty::ChartDate, "Last day, YYYY-MM-DD.").sent_as("chart_interval"),
            opt(
                "house",
                Ty::Str { max: 32 },
                "A house id from list_buildings, \"-1\" for charges \
                not tied to a house, or \"*\" for all (default).",
            )
            .sent_as("chart_house")
            .or(Val::Str("*")),
            opt(
                "service",
                Ty::Str { max: 32 },
                "A service id from list_services, or \"*\" for all \
                (default).",
            )
            .sent_as("chart_service")
            .or(Val::Str("*")),
        ],
        fixed: &[top("role_id", Val::Int(3))],
        owner: None,
    },
    Tool {
        name: "list_bank_accounts",
        description: "List a building manager's bank accounts (for get_cash_flow_chart).",
        access: Access::Read,
        path: BANK,
        action: "getBankAccountsList",
        shape: Shape::Plain,
        args: &[req("hk_id", Ty::Int, HK_ID).sent_as("role_owner_id")],
        // The dashboard asks as the housekeeper role (2), even for a resident.
        fixed: &[top("role_id", Val::Int(2))],
        owner: None,
    },
    Tool {
        name: "get_cash_flow_chart",
        description: "Get a building's cash flow (Рух коштів) from the reporting centre. \
                      chart_type 1: money in and out by month; 2: income by category; 3: \
                      expenses by category.",
        access: Access::Read,
        path: CHARTS,
        action: "initChart",
        shape: Shape::Plain,
        args: &[
            req("hk_id", Ty::Int, HK_ID).sent_as("role_owner_id"),
            req(
                "chart_type",
                Ty::Int,
                "1 money in and out, 2 income by category, 3 expenses by category.",
            ),
            req("from_date", Ty::ChartDate, "First day, YYYY-MM-DD.").sent_as("chart_interval"),
            req("to_date", Ty::ChartDate, "Last day, YYYY-MM-DD.").sent_as("chart_interval"),
            opt(
                "account",
                Ty::Str { max: 32 },
                "A bank account id from list_bank_accounts, or \
                \"*\" for all (default).",
            )
            .sent_as("chart_account")
            .or(Val::Str("*")),
        ],
        // mode 1 = banking stats.
        fixed: &[top("mode", Val::Int(1)), top("role_id", Val::Int(3))],
        owner: None,
    },
    // --- Documents --------------------------------------------------------
    Tool {
        name: "list_documents",
        description: "List documents shared with you (files and contracts), newest first.",
        access: Access::Read,
        path: FILES,
        action: "getHousekeeperFilesPagination",
        shape: Shape::Paginated,
        args: &[],
        fixed: &[
            additional("role_owner_id", Val::UserId),
            additional("role_id", Val::Str(ROLE_PRIVATE)),
            // 0 = all: your own documents, shared with you, and to sign.
            additional("source_id", Val::Int(0)),
            MY_ROLE,
            MY_ID,
            filter("source_id", Val::Str("0")),
        ],
        owner: None,
    },
    Tool {
        name: "list_statements",
        description: "List the statements (довідки) you have generated, newest first.",
        access: Access::Read,
        path: STATEMENTS,
        action: "getStatementsPagination",
        shape: Shape::Paginated,
        args: &[],
        fixed: &[
            additional("role_owner_id", Val::UserId),
            additional("role_id", Val::Str(ROLE_PRIVATE)),
        ],
        owner: None,
    },
    // --- Support requests -------------------------------------------------
    Tool {
        name: "list_support_requests",
        description: "List your support requests (заявки) with their status and SLA, newest \
                      first. Status: 0 new, 1 in progress, 2 waiting for your reply, 3 closed.",
        access: Access::Read,
        path: SUP,
        action: "getSupportRequestsPagination",
        shape: Shape::Paginated,
        args: &[],
        fixed: &[MY_ROLE, MY_ID, filter("filterByUser", Val::UserIdStr)],
        owner: Some("/user/id"),
    },
    Tool {
        name: "get_support_request_comments",
        description: "Get the comments on one support request.",
        access: Access::Read,
        path: SUP,
        action: "getCommentsByRequestId",
        shape: Shape::Plain,
        args: &[req("id", Ty::Int, "Support request id.")],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "list_support_recipients",
        description: "List the building managers you can send a support request to (their \
                      ids are the `hk_id` of the other support tools).",
        access: Access::Read,
        path: SUP,
        action: "getUserHouseKepers",
        shape: Shape::Plain,
        args: &[],
        fixed: &[top("user_id", Val::UserId)],
        owner: None,
    },
    Tool {
        name: "list_support_categories",
        description: "List the support request categories of a building manager. Category \
                      -1 (\"Інше\", other) is always allowed too.",
        access: Access::Read,
        path: SUP,
        action: "getSupportRequestCategoryByOwnerIdAndUserType",
        shape: Shape::Plain,
        args: &[req("hk_id", Ty::Int, HK_ID).sent_as("id")],
        // 1 = the applicant is a private person (a resident).
        fixed: &[top("type", Val::Int(1))],
        owner: None,
    },
    Tool {
        name: "list_support_subcategories",
        description: "List the sub-categories of a support category. Sub-category -1 (\"Інше\") \
                      is always allowed too. A sub-category with `schedule_settings` needs a \
                      time slot (get_support_schedule).",
        access: Access::Read,
        path: SUP,
        action: "getSupportSubCategoryByCategoryId",
        shape: Shape::Plain,
        args: &[req(
            "category_id",
            Ty::Int,
            "Category id from list_support_categories.",
        )
        .sent_as("id")],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "list_support_properties",
        description: "List your properties known to a building manager, grouped by address \
                      type (0 residence, 1 rented, 2 owned, 3 registration, 4 property rights). \
                      A request's `property_id` is \"<address_id>_<address_apartment>\".",
        access: Access::Read,
        path: SUP,
        action: "getUserPropertyByUserIdHkId",
        shape: Shape::Plain,
        args: &[req("hk_id", Ty::Int, HK_ID).sent_as("hkId")],
        fixed: &[top("userId", Val::UserId)],
        owner: None,
    },
    Tool {
        name: "get_support_schedule",
        description: "Get booked visit slots of a sub-category with `schedule_settings`: one \
                      entry per booked hour with its `count`; an hour is full when `count` \
                      reaches the sub-category's `depth`.",
        access: Access::Read,
        path: SUP,
        action: "getSupportSubCategoryScheduleMap",
        shape: Shape::Plain,
        args: &[
            req("sub_category_id", Ty::Int, "Sub-category id."),
            req("from_date", Ty::Date, "First day, YYYY-MM-DD."),
            req(
                "to_date",
                Ty::Date,
                "Last day, YYYY-MM-DD (the dashboard shows 7 days).",
            ),
        ],
        fixed: &[],
        owner: None,
    },
    // --- Messages and news ------------------------------------------------
    Tool {
        name: "list_messages",
        description: "List messages in your inbox, newest first. `read_date` empty = unread. \
                      Use read_message with `message_id` and `id` to open one.",
        access: Access::Read,
        path: MSG,
        action: "getMessagesPagination",
        shape: Shape::Paginated,
        args: &[],
        fixed: &[
            additional("role_owner_id", Val::UserId),
            additional("role_id", Val::Str(ROLE_PRIVATE)),
            additional("folder", Val::Int(0)),
        ],
        owner: None,
    },
    Tool {
        name: "list_archived_messages",
        description: "List messages in your archive, newest first.",
        access: Access::Read,
        path: MSG,
        action: "getMessagesPagination",
        shape: Shape::Paginated,
        args: &[],
        fixed: &[
            additional("role_owner_id", Val::UserId),
            additional("role_id", Val::Str(ROLE_PRIVATE)),
            additional("folder", Val::Int(1)),
        ],
        owner: None,
    },
    Tool {
        name: "count_unread_messages",
        description: "Count your unread messages.",
        access: Access::Read,
        path: MSG,
        action: "countUnreadMessages",
        shape: Shape::Plain,
        args: &[],
        fixed: &[
            top("role_owner_id", Val::UserId),
            top("role_id", Val::Str(ROLE_PRIVATE)),
        ],
        owner: None,
    },
    Tool {
        name: "list_service_messages",
        description: "List CheckDom service messages (system notices) for your account.",
        access: Access::Read,
        path: SERVICE_MSG,
        action: "getServiceMessagesByUserId",
        shape: Shape::Plain,
        args: &[],
        fixed: &[top("id", Val::UserId)],
        owner: None,
    },
    Tool {
        name: "list_news",
        description: "List news from your building managers and CheckDom, newest first. Pages \
                      by offset: pass the number of items already read as `offset`.",
        access: Access::Read,
        path: NEWS,
        action: "getNewsPseudoPagination",
        shape: Shape::Plain,
        args: &[
            opt("offset", Ty::Int, "Items to skip (default 0).")
                .sent_as("shown")
                .or(Val::Int(0)),
            opt("search", Ty::Str { max: 200 }, "Search in titles.").sent_as("searchName"),
            opt(
                "from_date",
                Ty::Date,
                "Only news from this day, YYYY-MM-DD.",
            )
            .sent_as("searchStartDate"),
            opt("to_date", Ty::Date, "Only news up to this day, YYYY-MM-DD.")
                .sent_as("searchEndDate"),
        ],
        fixed: &[top("userId", Val::UserId)],
        owner: None,
    },
    // --- Polls and votes --------------------------------------------------
    Tool {
        name: "list_voting_invitations",
        description: "List the polls and votes (голосування, опитування) you are invited to \
                      take part in. Status 2 = finished.",
        access: Access::Read,
        path: VOTE,
        action: "getVotingRequests",
        shape: Shape::Plain,
        args: &[],
        fixed: &[
            top("type", Val::Int(0)),
            top("role_owner_id", Val::UserId),
            top("role_id", Val::Str(ROLE_PRIVATE)),
        ],
        owner: None,
    },
    Tool {
        name: "list_votings",
        description: "List polls you created or manage, newest first. Status: 0 created, 1 \
                      running, 2 finished.",
        access: Access::Read,
        path: VOTE,
        action: "getVotingsPagination",
        shape: Shape::Paginated,
        args: &[],
        fixed: &[MY_ROLE, MY_ID, filter("type", Val::Str("0"))],
        owner: None,
    },
    Tool {
        name: "get_voting",
        description: "Get one poll as a voter: its questions with proposition ids (for simple \
                      questions 0 = for, 1 = against, 2 = abstain), your own votes (`votes`) and, \
                      where visible, the results.",
        access: Access::Read,
        path: VOTE,
        action: "getVotingCard",
        shape: Shape::Plain,
        args: &[req("id", Ty::Int, "Poll id from list_voting_invitations.")],
        fixed: &[
            top("voter", Val::Bool(true)),
            top("role_owner_id", Val::UserId),
            top("role_id", Val::Str(ROLE_PRIVATE)),
        ],
        owner: None,
    },
    // --- Passes and cars --------------------------------------------------
    Tool {
        name: "list_temporary_passes",
        description: "List your temporary passes (перепустки), newest first. Type: 1 guest, 2 \
                      taxi, 3 delivery. `canceled` = 1 cancelled; `checked_datetime` set = used.",
        access: Access::Read,
        path: TEMP_PASS,
        action: "filterUserTemporaryPassPagination",
        shape: Shape::Paginated,
        args: &[],
        fixed: &[MY_ROLE, MY_ID],
        owner: None,
    },
    Tool {
        name: "get_temporary_pass",
        description: "Get one temporary pass. Its share link is https://checkdom.in.ua/pass/<token>.",
        access: Access::Read,
        path: TEMP_PASS,
        action: "getTemporaryPassModalData",
        shape: Shape::Plain,
        args: &[req(
            "pass_id",
            Ty::Int,
            "Pass id from list_temporary_passes.",
        )],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "list_pass_addresses",
        description: "List your addresses where passes can be issued (empty = your building \
                      manager does not use passes).",
        access: Access::Read,
        path: TEMP_PASS,
        action: "getUserApartments",
        shape: Shape::Plain,
        args: &[],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "list_access_keys",
        description: "List your access key orders (fobs, QR codes), newest first. Status: 1 \
                      registered, 2 payment, 3 processing, 4 delivery, 5 cancelled.",
        access: Access::Read,
        path: KEYS,
        action: "filterUserLongtermPassPagination",
        shape: Shape::Paginated,
        args: &[],
        fixed: &[MY_ROLE, MY_ID],
        owner: None,
    },
    Tool {
        name: "list_cars",
        description: "List your cars registered for parking access.",
        access: Access::Read,
        path: CARS,
        action: "carsPagePagination",
        shape: Shape::Paginated,
        args: &[],
        fixed: &[MY_ROLE, MY_ID],
        owner: None,
    },
    Tool {
        name: "get_car",
        description: "Get one of your cars.",
        access: Access::Read,
        path: CARS,
        action: "getCarModalData",
        shape: Shape::Plain,
        args: &[req("id", Ty::Int, "Car id from list_cars.")],
        fixed: &[],
        owner: None,
    },
    // --- Writes (only with --allow-writes) --------------------------------
    Tool {
        name: "read_message",
        description: "Open one message and return its full text (HTML). WRITE action: CheckDom \
                      marks the message as read.",
        access: Access::Write,
        path: MSG,
        action: "viewMessage",
        shape: Shape::Plain,
        args: &[
            req("message_id", Ty::Int, "`message_id` from list_messages.").sent_as("id"),
            req("id", Ty::Int, "`id` (the inbox record) from list_messages.").sent_as("record_id"),
        ],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "create_support_request",
        description: "Create a new support request (заявка). WRITE action: confirm the details \
                      with the user first. Get `hk_id` from list_support_recipients, `property_id` \
                      and `property_type` from list_support_properties, `category` from \
                      list_support_categories, `sub_category` from list_support_subcategories \
                      (-1 = \"Інше\" for either).",
        access: Access::Write,
        path: SUP,
        action: "saveSupportRequest",
        shape: Shape::Plain,
        args: &[
            req("hk_id", Ty::Int, HK_ID).sent_as("support_id"),
            req(
                "property_id",
                Ty::Str { max: 64 },
                "\"<address_id>_<address_apartment>\".",
            ),
            req(
                "property_type",
                Ty::Int,
                "The property's address type, 0–4.",
            ),
            req("category", Ty::Int, "Category id, or -1 for other."),
            req("sub_category", Ty::Int, "Sub-category id, or -1 for other."),
            req(
                "description",
                Ty::Str { max: 999 },
                "What is wrong (1–999 characters).",
            ),
            opt(
                "subscriber_id",
                Ty::Int,
                "Your account id with this building manager, if known.",
            )
            .or(Val::Int(0)),
            opt(
                "schedule",
                Ty::TimeSlot,
                "One-hour visit slot {start_datetime, end_datetime} (\"YYYY-MM-DD HH:00:00\") \
                 when the sub-category requires one, else null.",
            )
            .or(Val::Null),
        ],
        fixed: &[
            top("id", Val::Int(-1)),
            top("user_id", Val::UserId),
            top("author_user_id", Val::UserId),
            // 1 = to the building manager (0 would go to CheckDom itself).
            top("type", Val::Int(1)),
            top("files", Val::Str("[]")),
            top("status", Val::Int(0)),
            top("comment_type", Val::Int(0)),
            top("request_entity_type", Val::Int(1)),
            top("request_entity_id", Val::Null),
        ],
        owner: None,
    },
    Tool {
        name: "comment_on_support_request",
        description: "Add a comment to an existing support request. WRITE action: the comment \
                      is visible to building staff; confirm the text with the user first. Set \
                      `resume` when answering a request that waits for your reply (status 2): \
                      it moves the request back to \"in progress\".",
        access: Access::Write,
        path: SUP,
        action: "saveComment",
        shape: Shape::Plain,
        args: &[
            req("request_id", Ty::Int, "Support request id."),
            req("comment_text", Ty::Str { max: 2000 }, "The comment."),
            opt(
                "resume",
                Ty::Bool,
                "Move a request waiting for your reply back to in progress.",
            ),
        ],
        fixed: &[
            top("user_id", Val::UserId),
            top("comment_type", Val::Int(0)),
            top("comment_files", Val::Str("[]")),
        ],
        owner: None,
    },
    Tool {
        name: "close_support_request",
        description: "Close one of your support requests as cancelled by you. WRITE action: \
                      it cannot be reopened from the dashboard; confirm with the user first.",
        access: Access::Write,
        path: SUP,
        action: "changeStatus",
        shape: Shape::Plain,
        args: &[req("id", Ty::Int, "Support request id.")],
        // status 3 = closed; closed_status 2 = cancelled by the client.
        fixed: &[
            top("status", Val::Int(3)),
            top("closed_status", Val::Int(2)),
        ],
        owner: None,
    },
    Tool {
        name: "vote",
        description: "Cast your vote on one question of a poll. WRITE action, final: confirm \
                      the choice with the user first. Ids come from get_voting.",
        access: Access::Write,
        path: VOTE,
        action: "vote",
        shape: Shape::Plain,
        args: &[
            req("voting_id", Ty::Int, "Poll id."),
            req("question_id", Ty::Int, "Question id."),
            req(
                "proposition_id",
                Ty::Int,
                "The chosen proposition's id (not its code).",
            ),
        ],
        fixed: &[
            top("role_owner_id", Val::UserId),
            top("role_id", Val::Str(ROLE_PRIVATE)),
        ],
        owner: None,
    },
    Tool {
        name: "get_pay_link",
        description: "Create a payment link for an amount on one of your personal accounts, \
                      for the user to open and pay themselves. WRITE action: it registers a \
                      pending order on CheckDom; nothing is charged until the user pays on the \
                      provider's page. Confirm the amount with the user first. `account_id` and \
                      `hk_id` come from get_property / get_balance; `provider` must be one of \
                      list_payment_providers (LiqPay or monobank).",
        access: Access::Write,
        path: IDX,
        action: "placeOrder",
        shape: Shape::PayLink,
        args: &[
            req("hk_id", Ty::Int, HK_ID),
            req(
                "account_id",
                Ty::Int,
                "Personal account id (get_property `accounts[].id`).",
            ),
            req("amount", Ty::Amount, "Amount in UAH, e.g. 1250.50."),
            req(
                "provider",
                Ty::OneOf(&["liqpay", "mono"]),
                "\"liqpay\" or \"mono\".",
            ),
            opt("invoice_id", Ty::Int, "The invoice this pays, if any.").sent_as("ref_invoice_id"),
        ],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "create_statement",
        description: "Generate a statement of settlements (довідка про стан розрахунків) for one \
                      property. WRITE action: it stores a new document in your account; it then \
                      appears in list_statements.",
        access: Access::Write,
        path: STATEMENTS,
        action: "getAccountingStatusPDF",
        shape: Shape::Plain,
        args: &[
            req(
                "property_id",
                Ty::Int,
                "Property `id` from list_properties.",
            )
            .sent_as("user_property_apartment_id"),
            opt("mode", Ty::Int, "1 owned (default), 2 rented.").or(Val::Int(1)),
        ],
        fixed: &[],
        owner: None,
    },
    Tool {
        name: "cancel_temporary_pass",
        description: "Cancel one of your temporary passes. WRITE action: confirm with the user \
                      first.",
        access: Access::Write,
        path: TEMP_PASS,
        action: "cancelPass",
        shape: Shape::Plain,
        args: &[req(
            "pass_id",
            Ty::Int,
            "Pass id from list_temporary_passes.",
        )],
        fixed: &[],
        owner: None,
    },
];

fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

/// Whether `name` is a tool this server knows, read or write.
pub fn exists(name: &str) -> bool {
    find(name).is_some()
}

/// The tool list advertised over `tools/list`.
pub fn list(allow_writes: bool) -> Value {
    TOOLS
        .iter()
        .filter(|t| allow_writes || t.access == Access::Read)
        .map(describe)
        .collect()
}

fn describe(tool: &Tool) -> Value {
    let read = tool.access == Access::Read;
    json!({
        "name": tool.name,
        "description": tool.description,
        "inputSchema": input_schema(tool),
        // Hints let clients ask for stricter approval on writes.
        "annotations": {
            "readOnlyHint": read,
            "destructiveHint": false,
            "idempotentHint": read,
            "openWorldHint": true
        }
    })
}

fn input_schema(tool: &Tool) -> Value {
    let mut props = Map::new();
    let mut required = Vec::new();
    if tool.shape == Shape::Paginated {
        props.insert(
            "page".into(),
            json!({ "type": "integer", "minimum": 1, "default": 1 }),
        );
        props.insert(
            "per_page".into(),
            json!({ "type": "integer", "minimum": 1, "maximum": MAX_PER_PAGE, "default": DEFAULT_PER_PAGE }),
        );
    }
    for arg in tool.args {
        let mut schema = match arg.ty {
            Ty::Int => json!({ "type": "integer" }),
            Ty::Str { max } => json!({ "type": "string", "minLength": 1, "maxLength": max }),
            Ty::Bool => json!({ "type": "boolean" }),
            Ty::Date | Ty::ChartDate => json!({ "type": "string", "format": "date" }),
            Ty::Amount => {
                json!({ "type": "number", "exclusiveMinimum": 0, "maximum": MAX_PAY_AMOUNT })
            }
            Ty::OneOf(values) => json!({ "type": "string", "enum": values }),
            Ty::TimeSlot => json!({
                "type": ["object", "null"],
                "properties": {
                    "start_datetime": { "type": "string" },
                    "end_datetime": { "type": "string" }
                },
                "required": ["start_datetime", "end_datetime"],
                "additionalProperties": false
            }),
        };
        schema["description"] = json!(arg.description);
        props.insert(arg.name.into(), schema);
        if arg.required {
            required.push(arg.name);
        }
    }
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false
    })
}

/// Values the server fills in from the session.
#[derive(Debug, Clone, Copy)]
struct Session {
    user_id: i64,
}

fn needs_user_id(tool: &Tool) -> bool {
    let is_user = |v: Val| matches!(v, Val::UserId | Val::UserIdStr);
    tool.fixed.iter().any(|f| is_user(f.val))
        || tool.args.iter().any(|a| a.default.is_some_and(is_user))
        || tool.owner.is_some()
}

/// Dispatch a `tools/call`. Returns the pretty-printed JSON payload as text.
pub async fn call(name: &str, args: Value, allow_writes: bool) -> Result<String> {
    let tool = find(name).ok_or_else(|| invalid(format!("unknown tool: {name}")))?;
    // Gate writes, then validate, before reading the token or touching the network.
    if tool.access == Access::Write && !allow_writes {
        return Err(invalid(
            "write tools are disabled; restart the server with --allow-writes to enable them",
        ));
    }
    let args = validate(tool, args)?;
    let client = CheckDomClient::from_stored()?;
    let session = Session {
        user_id: if needs_user_id(tool) {
            client.user_id().await?
        } else {
            0
        },
    };
    let data = if tool.shape == Shape::PayLink {
        pay_link(&client, tool, args, session).await?
    } else {
        let params = build_params(tool, args, session);
        let data = client.rpc(tool.path, tool.action, params).await?;
        match tool.owner {
            Some(pointer) => keep_own_rows(data, pointer, session.user_id),
            None => data,
        }
    };
    serde_json::to_string_pretty(&strip_noise(data)).map_err(CheckDomError::from)
}

/// Drop table rows that belong to someone else (the owner's user id at
/// `pointer` differs from ours): a second line of defence behind the filters,
/// so only your own rows ever reach the model.
fn keep_own_rows(mut data: Value, pointer: &str, user_id: i64) -> Value {
    let me = user_id.to_string();
    let mine = |row: &Value| {
        row.pointer(pointer)
            .is_some_and(|id| id.as_i64() == Some(user_id) || id.as_str() == Some(me.as_str()))
    };
    let removed = match data.get_mut("result").and_then(Value::as_array_mut) {
        Some(rows) => {
            let before = rows.len();
            rows.retain(mine);
            before - rows.len()
        }
        None => 0,
    };
    if removed > 0
        && let Value::Object(map) = &mut data
    {
        map.insert(
            "note".into(),
            json!(format!("{removed} rows that are not yours were removed")),
        );
    }
    data
}

/// Create a pending order and return the provider's hosted payment page, the
/// same two steps as the dashboard's pay button. Nothing is charged here: the
/// user pays (or not) on the provider's page. `payByClientCard`, which charges
/// a saved card, is never called.
async fn pay_link(
    client: &CheckDomClient,
    tool: &Tool,
    valid: Valid,
    session: Session,
) -> Result<Value> {
    let provider = valid
        .args
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let placed = client
        .rpc(IDX, "placeOrder", order_params(tool, valid, session))
        .await?;
    let order_id = placed
        .get("order_id")
        .filter(|id| !id.is_null() && id.as_i64() != Some(-1))
        .cloned()
        .ok_or_else(|| invalid("CheckDom did not create the order"))?;

    let url = if provider == "mono" {
        let data = client
            .rpc(IDX, "getMonoParams", json!({ "order_id": order_id }))
            .await?;
        mono_url(&data)
    } else {
        let data = client
            .rpc(
                IDX,
                "getLiqpayWidgetParams",
                json!({ "order_id": order_id }),
            )
            .await?;
        liqpay_url(&data)
    }
    .ok_or_else(|| invalid("CheckDom did not return a payment page for the order"))?;

    Ok(json!({ "order_id": order_id, "provider": provider, "pay_url": url }))
}

/// `placeOrder` params: one payment position, as the dashboard sends it.
fn order_params(tool: &Tool, valid: Valid, session: Session) -> Value {
    let p = build_params(tool, valid, session);
    json!({ "params": [{
        "user_id": session.user_id,
        "account_id": p["account_id"].to_string(),
        "hk_id": p["hk_id"],
        "amount": p["amount"],
        "ref_invoice_id": p.get("ref_invoice_id").cloned().unwrap_or(json!(0)),
    }] })
}

/// monobank's hosted payment page from `getMonoParams`.
fn mono_url(data: &Value) -> Option<String> {
    let url = data.pointer("/params/0/page_url")?.as_str()?;
    url.starts_with("https://").then(|| url.to_string())
}

/// LiqPay's hosted checkout URL from `getLiqpayWidgetParams` (`form[0].data`/`sig`).
fn liqpay_url(data: &Value) -> Option<String> {
    let form = data.pointer("/form/0")?;
    let mut url = reqwest::Url::parse(LIQPAY_CHECKOUT).ok()?;
    url.query_pairs_mut()
        .append_pair("data", form.get("data")?.as_str()?)
        .append_pair("signature", form.get("sig")?.as_str()?);
    Some(url.into())
}

fn invalid(msg: impl Into<String>) -> CheckDomError {
    CheckDomError::Unexpected(msg.into())
}

/// Drop bookkeeping fields the dashboard adds to responses, and the pay token.
fn strip_noise(mut data: Value) -> Value {
    if let Value::Object(map) = &mut data {
        map.remove("curVersionTime");
        map.remove("langData");
        // A payment token for the dashboard's own pay widget; never handed to the model.
        map.remove("payToken");
    }
    data
}

/// Validated arguments: page / per_page (for tables) and the declared arguments.
#[derive(Debug)]
struct Valid {
    page: i64,
    per_page: i64,
    args: Map<String, Value>,
}

/// Check `args` against the tool's declared arguments. Unknown keys are
/// rejected, so a tool sends only the fields it documents.
fn validate(tool: &Tool, args: Value) -> Result<Valid> {
    let mut args = match args {
        Value::Object(map) => map,
        Value::Null => Map::new(),
        _ => return Err(invalid("arguments must be an object")),
    };
    let (mut page, mut per_page) = (1, DEFAULT_PER_PAGE);
    if tool.shape == Shape::Paginated {
        page = take_int_in(&mut args, "page", 1, 1, i64::MAX)?;
        per_page = take_int_in(&mut args, "per_page", DEFAULT_PER_PAGE, 1, MAX_PER_PAGE)?;
    }

    let mut valid = Map::new();
    for arg in tool.args {
        match args.remove(arg.name) {
            // An explicit `null` time slot means "no slot" and is sent as such.
            Some(Value::Null) if matches!(arg.ty, Ty::TimeSlot) => {
                valid.insert(arg.name.into(), Value::Null);
            }
            None | Some(Value::Null) => {
                if arg.required {
                    return Err(invalid(format!("missing argument: {}", arg.name)));
                }
            }
            Some(value) => {
                check_type(arg, &value)?;
                valid.insert(arg.name.into(), value);
            }
        }
    }
    if let Some(key) = args.keys().next() {
        return Err(invalid(format!(
            "unsupported argument for {}: {key}",
            tool.name
        )));
    }
    Ok(Valid {
        page,
        per_page,
        args: valid,
    })
}

fn resolve(val: Val, session: Session) -> Value {
    match val {
        Val::Int(n) => json!(n),
        Val::Str(s) => json!(s),
        Val::Bool(b) => json!(b),
        Val::Null => Value::Null,
        Val::UserId => json!(session.user_id),
        Val::UserIdStr => json!(session.user_id.to_string()),
    }
}

/// Build the RPC params from validated arguments, defaults and fixed fields.
fn build_params(tool: &Tool, valid: Valid, session: Session) -> Value {
    let mut params = Map::new();
    let mut additional = Map::new();
    let mut filters = Map::new();

    for arg in tool.args {
        let value = match valid.args.get(arg.name) {
            Some(v) => v.clone(),
            None => match arg.default {
                Some(d) => resolve(d, session),
                None => continue,
            },
        };
        if matches!(arg.ty, Ty::ChartDate) {
            let ua = value.as_str().map(to_ua_date).unwrap_or_default();
            match params.entry(arg.key).or_insert_with(|| json!([])) {
                Value::Array(dates) => dates.push(json!(ua)),
                other => *other = json!([ua]),
            }
        } else {
            params.insert(arg.key.into(), value);
        }
    }
    // `resume` is a flag, sent as the dashboard's `status: 1` only when set.
    if params.remove("resume") == Some(Value::Bool(true)) {
        params.insert("status".into(), json!(1));
    }
    for f in tool.fixed {
        let target = match f.place {
            Place::Top => &mut params,
            Place::Additional => &mut additional,
            Place::Filter => &mut filters,
        };
        target.insert(f.key.into(), resolve(f.val, session));
    }
    if tool.shape == Shape::Paginated {
        params.insert(
            "pagination".into(),
            json!({
                "page": valid.page,
                "perPage": valid.per_page.to_string(),
                "orderBy": { "param": "id", "dir": "desc" },
                "filters": filters
            }),
        );
        params.insert("additionalData".into(), Value::Object(additional));
    }
    Value::Object(params)
}

fn take_int_in(
    args: &mut Map<String, Value>,
    key: &str,
    default: i64,
    min: i64,
    max: i64,
) -> Result<i64> {
    let Some(value) = args.remove(key) else {
        return Ok(default);
    };
    value
        .as_i64()
        .filter(|n| (min..=max).contains(n))
        .ok_or_else(|| invalid(format!("{key} must be an integer from {min} to {max}")))
}

/// `YYYY-MM-DD` with a plausible month and day.
fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| s.get(r).and_then(|x| x.parse::<u32>().ok());
    matches!(
        (num(0..4), num(5..7), num(8..10)),
        (Some(y), Some(m), Some(d)) if y >= 2000 && (1..=12).contains(&m) && (1..=31).contains(&d)
    )
}

/// `YYYY-MM-DD` to the charts' `DD.MM.YYYY`.
fn to_ua_date(iso: &str) -> String {
    match (iso.get(0..4), iso.get(5..7), iso.get(8..10)) {
        (Some(y), Some(m), Some(d)) => format!("{d}.{m}.{y}"),
        _ => iso.to_string(),
    }
}

fn check_type(arg: &Arg, value: &Value) -> Result<()> {
    let ok = match arg.ty {
        Ty::Int => value.is_i64(),
        Ty::Str { max } => value
            .as_str()
            .is_some_and(|s| !s.trim().is_empty() && s.chars().count() <= max),
        Ty::Bool => value.is_boolean(),
        Ty::Date | Ty::ChartDate => value.as_str().is_some_and(is_date),
        Ty::Amount => value.as_f64().is_some_and(|a| {
            a > 0.0 && a <= MAX_PAY_AMOUNT && ((a * 100.0).round() - a * 100.0).abs() < 1e-6
        }),
        Ty::OneOf(values) => value.as_str().is_some_and(|s| values.contains(&s)),
        Ty::TimeSlot => match value {
            Value::Null => true,
            Value::Object(m) => {
                m.len() == 2
                    && ["start_datetime", "end_datetime"]
                        .iter()
                        .all(|k| m.get(*k).is_some_and(Value::is_string))
            }
            _ => false,
        },
    };
    if ok {
        Ok(())
    } else {
        Err(invalid(format!(
            "invalid argument {}: expected {}",
            arg.name,
            match arg.ty {
                Ty::Int => "an integer".to_string(),
                Ty::Str { max } => format!("a non-empty string of at most {max} characters"),
                Ty::Bool => "true or false".to_string(),
                Ty::Date | Ty::ChartDate => "a date YYYY-MM-DD".to_string(),
                Ty::Amount => format!(
                    "an amount in UAH above 0 and at most {MAX_PAY_AMOUNT}, with at most 2 decimals"
                ),
                Ty::OneOf(values) => format!("one of {}", values.join(", ")),
                Ty::TimeSlot => "{start_datetime, end_datetime} or null".to_string(),
            }
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: Session = Session { user_id: 4321 };

    /// Minimal valid arguments for `tool`: every required argument filled in.
    fn minimal_args(tool: &Tool) -> Value {
        let mut m = Map::new();
        for arg in tool.args.iter().filter(|a| a.required) {
            let v = match arg.ty {
                Ty::Int => json!(1),
                Ty::Str { .. } => json!("text"),
                Ty::Bool => json!(true),
                Ty::Date | Ty::ChartDate => json!("2026-11-02"),
                Ty::Amount => json!(10.5),
                Ty::OneOf(values) => json!(values[0]),
                Ty::TimeSlot => Value::Null,
            };
            m.insert(arg.name.into(), v);
        }
        Value::Object(m)
    }

    fn params(name: &str, args: Value) -> Value {
        let tool = find(name).unwrap();
        build_params(tool, validate(tool, args).unwrap(), SESSION)
    }

    fn names(v: &Value) -> Vec<&str> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn tool_names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        assert!(TOOLS.iter().all(|t| seen.insert(t.name)));
    }

    #[test]
    fn read_only_list_has_exactly_the_read_tools() {
        let reads: Vec<&str> = TOOLS
            .iter()
            .filter(|t| t.access == Access::Read)
            .map(|t| t.name)
            .collect();
        assert_eq!(names(&list(false)), reads);
        assert_eq!(names(&list(true)).len(), TOOLS.len());
    }

    #[tokio::test]
    async fn every_write_tool_is_refused_without_allow_writes() {
        for tool in TOOLS.iter().filter(|t| t.access == Access::Write) {
            let err = call(tool.name, minimal_args(tool), false)
                .await
                .unwrap_err();
            assert!(err.to_string().contains("--allow-writes"), "{}", tool.name);
        }
    }

    #[test]
    fn no_tool_reaches_a_payment_action() {
        // payByClientCard charges a saved card; it must never be reachable.
        for tool in TOOLS {
            assert_ne!(tool.action, "payByClientCard", "{}", tool.name);
        }
    }

    #[test]
    fn state_changing_actions_are_write_tools() {
        let writes = [
            "viewMessage",
            "saveSupportRequest",
            "saveComment",
            "changeStatus",
            "vote",
            "fullVote",
            "cancelPass",
            "savePass",
            "saveCar",
            "deleteCar",
            "placeOrder",
            "getAccountingStatusPDF",
        ];
        for tool in TOOLS.iter().filter(|t| writes.contains(&t.action)) {
            assert_eq!(tool.access, Access::Write, "{}", tool.name);
        }
    }

    #[test]
    fn annotations_follow_access() {
        for tool in TOOLS {
            let hints = &describe(tool)["annotations"];
            let read = tool.access == Access::Read;
            assert_eq!(hints["readOnlyHint"], read, "{}", tool.name);
            assert_eq!(hints["idempotentHint"], read, "{}", tool.name);
        }
    }

    #[test]
    fn schema_lists_exactly_the_accepted_arguments() {
        for tool in TOOLS {
            let schema = input_schema(tool);
            assert_eq!(schema["additionalProperties"], false);
            let props = schema["properties"].as_object().unwrap();
            let paged = usize::from(tool.shape == Shape::Paginated) * 2;
            assert_eq!(props.len(), tool.args.len() + paged, "{}", tool.name);
            // The minimal arguments the schema requires are accepted.
            validate(tool, minimal_args(tool)).unwrap();
        }
    }

    #[test]
    fn additional_data_and_filters_are_only_used_by_tables() {
        for tool in TOOLS {
            if tool
                .fixed
                .iter()
                .any(|f| matches!(f.place, Place::Additional | Place::Filter))
            {
                assert_eq!(tool.shape, Shape::Paginated, "{}", tool.name);
            }
        }
    }

    #[test]
    fn tables_that_list_people_are_limited_to_you() {
        // Every table that can contain other people's rows sends the dashboard's
        // own-rows filters.
        for name in [
            "list_support_requests",
            "list_temporary_passes",
            "list_access_keys",
            "list_cars",
            "list_votings",
            "list_documents",
        ] {
            let p = params(name, Value::Null);
            let f = &p["pagination"]["filters"];
            assert_eq!(f["role_id"], "3", "{name}");
            assert_eq!(f["role_owner_id"], "4321", "{name}");
        }
        let p = params("list_support_requests", Value::Null);
        assert_eq!(p["pagination"]["filters"]["filterByUser"], "4321");
    }

    #[test]
    fn rows_of_other_users_are_dropped() {
        let data = json!({ "total": 3, "result": [
            { "id": 1, "user": { "id": 4321 } },
            { "id": 2, "user": { "id": 999 } },
            { "id": 3, "user": { "id": "4321" } },
            { "id": 4 }
        ]});
        let kept = keep_own_rows(data, "/user/id", 4321);
        let ids: Vec<i64> = kept["result"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_i64().unwrap())
            .collect();
        assert_eq!(ids, [1, 3]);
        assert!(kept["note"].as_str().unwrap().contains("2 rows"));
        let clean = keep_own_rows(
            json!({ "result": [{ "user": { "id": 4321 } }] }),
            "/user/id",
            4321,
        );
        assert!(clean.get("note").is_none());
    }

    #[test]
    fn unknown_arguments_are_rejected_for_every_tool() {
        for tool in TOOLS {
            for (key, value) in [
                ("action", json!("changeStatus")),
                ("filters", json!({ "user_id": 2 })),
                ("user_id", json!(2)),
                ("role_owner_id", json!(2)),
            ] {
                let mut args = minimal_args(tool);
                args[key] = value;
                assert!(
                    validate(tool, args).is_err(),
                    "{} accepted {key}",
                    tool.name
                );
            }
        }
    }

    #[test]
    fn fixed_fields_and_the_session_cannot_be_overridden() {
        // Fixed fields are not arguments, so they are rejected as unknown.
        let tool = find("close_support_request").unwrap();
        assert!(validate(tool, json!({ "id": 1, "status": 0 })).is_err());
        let tool = find("comment_on_support_request").unwrap();
        let args = json!({ "request_id": 1, "comment_text": "Дякую", "comment_type": 5 });
        assert!(validate(tool, args).is_err());
    }

    #[test]
    fn missing_or_mistyped_arguments_are_rejected() {
        let tool = find("get_debt_status").unwrap();
        assert!(validate(tool, json!({})).is_err());
        assert!(validate(tool, json!({ "hk_id": "7" })).is_err());
        assert!(validate(tool, json!({ "hk_id": 7, "house_id": { "$ne": 0 } })).is_err());
        assert!(validate(tool, json!({ "hk_id": 7, "section": ["a"] })).is_err());
        assert!(validate(tool, json!([7])).is_err());
        let tool = find("get_support_schedule").unwrap();
        let args =
            json!({ "sub_category_id": 1, "from_date": "02.11.2026", "to_date": "2026-11-08" });
        assert!(validate(tool, args).is_err());
    }

    #[test]
    fn debt_status_defaults_to_all() {
        // Regression: omitting the filters made the server answer HTTP 500.
        assert_eq!(
            params("get_debt_status", json!({ "hk_id": 7 })),
            json!({ "hk_id": 7, "house_id": "*", "section": "*", "floor": "*" })
        );
        assert_eq!(
            params("get_debt_status", json!({ "hk_id": 7, "floor": "3" }))["floor"],
            "3"
        );
    }

    #[test]
    fn tables_send_the_dashboards_pagination_shape() {
        let p = params("list_invoices", Value::Null);
        assert_eq!(
            p,
            json!({
                "pagination": {
                    "page": 1, "perPage": "25",
                    "orderBy": { "param": "id", "dir": "desc" },
                    "filters": {}
                },
                "additionalData": {}
            })
        );
        let p = params("list_invoices", json!({ "page": 3, "per_page": 100 }));
        assert_eq!(p["pagination"]["page"], 3);
        assert_eq!(p["pagination"]["perPage"], "100");

        let tool = find("list_invoices").unwrap();
        for bad in [
            json!({ "page": 0 }),
            json!({ "per_page": 101 }),
            json!({ "page": "2" }),
        ] {
            assert!(validate(tool, bad).is_err());
        }
    }

    #[test]
    fn messages_send_role_and_folder_in_additional_data() {
        // Regression: role ids at the top level made the server answer HTTP 500.
        let inbox = params("list_messages", Value::Null);
        assert_eq!(
            inbox["additionalData"],
            json!({ "role_owner_id": 4321, "role_id": "3", "folder": 0 })
        );
        assert!(inbox.get("role_id").is_none());
        let archive = params("list_archived_messages", Value::Null);
        assert_eq!(archive["additionalData"]["folder"], 1);
    }

    #[test]
    fn renamed_arguments_are_sent_under_the_api_name() {
        let p = params("list_support_properties", json!({ "hk_id": 9 }));
        assert_eq!(p, json!({ "hkId": 9, "userId": 4321 }));
        let p = params("read_message", json!({ "message_id": 5, "id": 6 }));
        assert_eq!(p, json!({ "id": 5, "record_id": 6 }));
        let p = params("list_news", Value::Null);
        assert_eq!(p, json!({ "shown": 0, "userId": 4321 }));
    }

    #[test]
    fn support_request_matches_the_dashboard() {
        let p = params(
            "create_support_request",
            json!({
                "hk_id": 3, "property_id": "123_45", "property_type": 2,
                "category": -1, "sub_category": -1,
                "description": "Лампа в під'їзді не світить"
            }),
        );
        assert_eq!(p["support_id"], 3);
        assert_eq!(p["property_id"], "123_45");
        assert_eq!(p["id"], -1);
        assert_eq!(p["user_id"], 4321);
        assert_eq!(p["author_user_id"], 4321);
        assert_eq!(p["type"], 1);
        assert_eq!(p["files"], "[]");
        assert_eq!(p["status"], 0);
        assert_eq!(p["request_entity_type"], 1);
        assert!(p["request_entity_id"].is_null());
        assert!(p["schedule"].is_null());
        assert_eq!(p["subscriber_id"], 0);

        let tool = find("create_support_request").unwrap();
        let base = json!({
            "hk_id": 3, "property_id": "123_45", "property_type": 2,
            "category": 1, "sub_category": 1, "description": "x"
        });
        let mut v = base.clone();
        v["description"] = json!("я".repeat(1000));
        assert!(validate(tool, v).is_err());
        let mut v = base.clone();
        v["schedule"] = json!({ "start_datetime": "x" });
        assert!(validate(tool, v).is_err());
        let mut v = base;
        v["property_id"] = json!(123);
        assert!(validate(tool, v).is_err(), "property_id is a string");
    }

    #[test]
    fn comment_resume_flag_becomes_status_1() {
        let base = json!({ "request_id": 1, "comment_text": "Дякую" });
        let p = params("comment_on_support_request", base.clone());
        assert!(p.get("status").is_none());
        assert!(p.get("resume").is_none());
        assert_eq!(p["comment_files"], "[]");
        assert_eq!(p["user_id"], 4321);
        let mut with = base;
        with["resume"] = json!(true);
        assert_eq!(params("comment_on_support_request", with)["status"], 1);
    }

    #[test]
    fn dates_are_checked() {
        assert!(is_date("2026-11-02"));
        for bad in [
            "2026-13-01",
            "2026-11-32",
            "26-11-02",
            "2026/11/02",
            "02.11.2026",
            "",
        ] {
            assert!(!is_date(bad), "{bad}");
        }
    }

    #[test]
    fn noise_fields_and_pay_token_are_stripped() {
        let data = json!({ "result": [], "curVersionTime": 1, "langData": {}, "payToken": "t" });
        assert_eq!(strip_noise(data), json!({ "result": [] }));
    }

    #[test]
    fn chart_dates_become_a_ua_interval() {
        let p = params(
            "get_settlements_chart",
            json!({ "hk_id": 5, "chart_type": 0, "from_date": "2025-10-31", "to_date": "2026-10-31" }),
        );
        assert_eq!(p["chart_interval"], json!(["31.10.2025", "31.10.2026"]));
        assert_eq!(p["role_owner_id"], 5);
        assert_eq!(p["role_id"], 3);
        assert_eq!(p["chart_house"], "*");
        assert_eq!(p["chart_service"], "*");
        assert!(p.get("from_date").is_none());
    }

    #[test]
    fn amounts_are_checked() {
        let tool = find("get_pay_link").unwrap();
        let base = json!({ "hk_id": 1, "account_id": 2, "amount": 1250.5, "provider": "mono" });
        assert!(validate(tool, base.clone()).is_ok());
        for bad in [
            json!(0),
            json!(-5),
            json!(10.005),
            json!("100"),
            json!(2_000_000),
        ] {
            let mut v = base.clone();
            v["amount"] = bad.clone();
            assert!(validate(tool, v).is_err(), "{bad}");
        }
        let mut v = base;
        v["provider"] = json!("card");
        assert!(validate(tool, v).is_err(), "only liqpay or mono");
    }

    #[test]
    fn pay_order_matches_the_dashboard() {
        let tool = find("get_pay_link").unwrap();
        let valid = validate(
            tool,
            json!({ "hk_id": 7, "account_id": 88, "amount": 99.9, "provider": "liqpay" }),
        )
        .unwrap();
        assert_eq!(
            order_params(tool, valid, SESSION),
            json!({ "params": [{ "user_id": 4321, "account_id": "88", "hk_id": 7,
                                 "amount": 99.9, "ref_invoice_id": 0 }] })
        );
    }

    #[test]
    fn pay_urls_are_built_from_provider_data() {
        let mono = json!({ "params": [{ "page_url": "https://pay.monobank.ua/abc" }] });
        assert_eq!(
            mono_url(&mono).as_deref(),
            Some("https://pay.monobank.ua/abc")
        );
        assert_eq!(
            mono_url(&json!({ "params": [{ "page_url": "javascript:alert(1)" }] })),
            None
        );
        let liq = json!({ "form": [{ "data": "eyJh+/=", "sig": "s/g=" }] });
        assert_eq!(
            liqpay_url(&liq).as_deref(),
            Some("https://www.liqpay.ua/api/3/checkout?data=eyJh%2B%2F%3D&signature=s%2Fg%3D")
        );
        assert_eq!(liqpay_url(&json!({})), None);
    }
}
