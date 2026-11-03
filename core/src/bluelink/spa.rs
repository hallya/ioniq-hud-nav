//! Legacy SPA vehicle API on `ccapi:8080` (upstream `ApiImplType1`):
//! device registration, vehicle list, state of charge, PIN control token,
//! send destination.
//!
//! Headers follow upstream `_get_authenticated_headers` minus `Host`,
//! `Connection` and `Accept-Encoding`, which URLSession sets itself.

use super::stamp::stamp;
use super::{AppConstants, Error, Token};
use crate::http::{Method, Request, Response};
use serde_json::Value;

/// What every authenticated SPA call needs.
#[derive(Debug, Clone, Copy)]
pub struct Session<'a> {
    /// Raw CCS token from [`super::cci::parse_ccs`].
    pub access_token: &'a str,
    pub device_id: &'a str,
    /// Current Unix time in seconds, for the `Stamp` header.
    pub now_s: u64,
}

/// A vehicle of the account. The VIN is deliberately not kept.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Vehicle {
    pub id: String,
    pub nickname: String,
    pub model: String,
    /// `ccuCCS2ProtocolSupport`: 0 legacy status, otherwise CCS2.
    pub ccs2: u8,
}

/// A destination for the car's navigation.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Poi {
    pub lat: f64,
    pub lon: f64,
    pub name: String,
    pub addr: String,
    pub zip: String,
}

/// `push_reg_id`: 64 random hex characters; `uuid`: a random UUID.
pub fn register_request(c: &AppConstants, now_s: u64, push_reg_id: &str, uuid: &str) -> Request {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Body<'a> {
        push_reg_id: &'a str,
        push_type: &'a str,
        uuid: &'a str,
    }
    let body = Body {
        push_reg_id,
        push_type: &c.push_type,
        uuid,
    };
    Request::new(
        Method::Post,
        format!("{}/api/v1/spa/notifications/register", c.spa_host),
    )
    .header("ccsp-service-id", &c.ccsp_service_id)
    .header("ccsp-application-id", &c.app_id)
    .header("Stamp", stamp(c, now_s))
    .header("Content-Type", "application/json;charset=UTF-8")
    .header("User-Agent", &c.spa_user_agent)
    .body(to_json(&body))
}

pub fn parse_device_id(r: &Response) -> Result<String, Error> {
    let envelope = envelope(r)?;
    envelope.res_msg["deviceId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or(Error::Parse("device"))
}

pub fn vehicles_request(c: &AppConstants, s: &Session) -> Request {
    Request::new(Method::Get, format!("{}/api/v1/spa/vehicles", c.spa_host))
        .headers(authenticated_headers(c, s, s.access_token, 0))
}

pub fn parse_vehicles(r: &Response) -> Result<Vec<Vehicle>, Error> {
    let envelope = envelope(r)?;
    let list = envelope.res_msg["vehicles"]
        .as_array()
        .ok_or(Error::Parse("vehicles"))?;
    list.iter()
        .map(|v| {
            let text = |key: &str| v[key].as_str().unwrap_or("").to_string();
            let id = text("vehicleId");
            if id.is_empty() {
                return Err(Error::Parse("vehicles"));
            }
            Ok(Vehicle {
                id,
                nickname: text("nickname"),
                model: text("vehicleName"),
                ccs2: v["ccuCCS2ProtocolSupport"]
                    .as_u64()
                    .and_then(|n| u8::try_from(n).ok())
                    .unwrap_or(0),
            })
        })
        .collect()
}

/// Latest cached status; does not wake the car.
pub fn status_request(c: &AppConstants, s: &Session, v: &Vehicle) -> Request {
    let path = if v.ccs2 == 0 {
        "status/latest"
    } else {
        "ccs2/carstatus/latest"
    };
    Request::new(
        Method::Get,
        format!("{}/api/v1/spa/vehicles/{}/{path}", c.spa_host, v.id),
    )
    .headers(authenticated_headers(c, s, s.access_token, v.ccs2))
}

/// High-voltage battery state of charge, in percent. Values outside
/// 0..=100 are the car's "unknown" sentinels, not a charge.
pub fn parse_soc(r: &Response, ccs2: u8) -> Result<u8, Error> {
    let envelope = envelope(r)?;
    let value = if ccs2 == 0 {
        &envelope.res_msg["vehicleStatusInfo"]["vehicleStatus"]["evStatus"]["batteryStatus"]
    } else {
        &envelope.res_msg["state"]["Vehicle"]["Green"]["BatteryManagement"]["BatteryRemain"]
            ["Ratio"]
    };
    value
        .as_f64()
        .filter(|v| (0.0..=100.0).contains(v))
        .map(|v| v.floor() as u8)
        .ok_or(Error::Parse("soc"))
}

/// Trades the Bluelink PIN for a short-lived control token.
pub fn pin_request(c: &AppConstants, s: &Session, pin: &str) -> Request {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Body<'a> {
        device_id: &'a str,
        pin: &'a str,
    }
    let body = Body {
        device_id: s.device_id,
        pin,
    };
    Request::new(
        Method::Put,
        format!("{}/api/v1/user/pin?token=", c.spa_host),
    )
    .header("Authorization", format!("Bearer {}", s.access_token))
    .header("Content-type", "application/json")
    .header("User-Agent", &c.spa_user_agent)
    .body(to_json(&body))
}

pub fn parse_control_token(r: &Response) -> Result<Token, Error> {
    if r.status >= 500 {
        return Err(Error::Http(r.status));
    }
    let body: Value = serde_json::from_str(&r.body).unwrap_or(Value::Null);
    let token = body["controlToken"]
        .as_str()
        .filter(|t| !t.is_empty())
        .ok_or(Error::Pin)?;
    Ok(Token {
        access_token: token.to_string(),
        expires_in_s: body["expiresTime"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0),
    })
}

/// One or more waypoints, in order; the last is the destination.
pub fn send_destination_request(
    c: &AppConstants,
    s: &Session,
    control: &Token,
    v: &Vehicle,
    pois: &[Poi],
) -> Request {
    #[derive(serde::Serialize)]
    struct Coord {
        lat: f64,
        alt: u8,
        lon: f64,
        #[serde(rename = "type")]
        kind: u8,
    }
    #[derive(serde::Serialize)]
    struct PoiInfo<'a> {
        phone: &'a str,
        #[serde(rename = "waypointID")]
        waypoint_id: usize,
        lang: u8,
        src: &'a str,
        coord: Coord,
        addr: &'a str,
        zip: &'a str,
        placeid: &'a str,
        name: &'a str,
    }
    #[derive(serde::Serialize)]
    struct Body<'a> {
        #[serde(rename = "deviceID")]
        device_id: &'a str,
        #[serde(rename = "poiInfoList")]
        poi_info_list: Vec<PoiInfo<'a>>,
    }
    let body = Body {
        device_id: s.device_id,
        poi_info_list: pois
            .iter()
            .enumerate()
            .map(|(i, p)| PoiInfo {
                phone: "",
                waypoint_id: i + 1,
                lang: 1,
                src: "HERE",
                coord: Coord {
                    lat: p.lat,
                    alt: 0,
                    lon: p.lon,
                    kind: 0,
                },
                addr: &p.addr,
                zip: &p.zip,
                placeid: "",
                name: &p.name,
            })
            .collect(),
    };
    let mut headers = authenticated_headers(c, s, &control.access_token, v.ccs2);
    headers.push((
        "AuthorizationCCSP".into(),
        format!("Bearer {}", control.access_token),
    ));
    headers.push(("Content-Type".into(), "application/json".into()));
    Request::new(
        Method::Post,
        format!(
            "{}/api/v2/spa/vehicles/{}/location/routes",
            c.spa_host, v.id
        ),
    )
    .headers(headers)
    .body(to_json(&body))
}

/// The message id of an accepted command.
pub fn parse_msg_id(r: &Response) -> Result<String, Error> {
    let envelope = envelope(r)?;
    envelope
        .msg_id
        .filter(|id| !id.is_empty())
        .ok_or(Error::Parse("msg"))
}

/// Upstream `_get_authenticated_headers`, minus what URLSession sets.
fn authenticated_headers(
    c: &AppConstants,
    s: &Session,
    bearer: &str,
    ccs2: u8,
) -> Vec<(String, String)> {
    [
        ("Authorization", format!("Bearer {bearer}")),
        ("ccsp-service-id", c.ccsp_service_id.clone()),
        ("ccsp-application-id", c.app_id.clone()),
        ("Stamp", stamp(c, s.now_s)),
        ("ccsp-device-id", s.device_id.to_string()),
        ("Ccuccs2protocolsupport", ccs2.to_string()),
        ("User-Agent", c.spa_user_agent.clone()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

struct Envelope {
    res_msg: Value,
    msg_id: Option<String>,
}

/// Upstream `_check_response_for_errors`: `retCode` F carries a `resCode`,
/// an `error` key means the token is no longer accepted.
fn envelope(r: &Response) -> Result<Envelope, Error> {
    let Ok(body) = serde_json::from_str::<Value>(&r.body) else {
        return Err(if r.is_success() {
            Error::Parse("spa")
        } else {
            Error::Http(r.status)
        });
    };
    if body["retCode"] == "F" {
        let code = match &body["resCode"] {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        };
        return Err(Error::Api(code));
    }
    if body.get("error").is_some() {
        return Err(Error::TokenExpired);
    }
    if !r.is_success() {
        return Err(Error::Http(r.status));
    }
    if body.get("resMsg").is_none() {
        return Err(Error::Parse("spa"));
    }
    Ok(Envelope {
        msg_id: body["msgId"].as_str().map(str::to_string),
        res_msg: body["resMsg"].clone(),
    })
}

fn to_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("request bodies are plain structs")
}

#[cfg(test)]
mod tests {
    use super::super::fixtures;
    use super::*;
    use serde_json::Value;

    fn with_session(f: &Value, run: impl FnOnce(&Session)) {
        let i = &f["inputs"];
        run(&Session {
            access_token: fixtures::str(i, "access_token"),
            device_id: fixtures::str(i, "device_id"),
            now_s: i["now_s"].as_u64().expect("now_s"),
        });
    }

    fn vehicle(id: &str, ccs2: u8) -> Vehicle {
        Vehicle {
            id: id.into(),
            nickname: String::new(),
            model: String::new(),
            ccs2,
        }
    }

    #[test]
    fn register() {
        let f = fixtures::json("spa.json");
        let step = &f["register"];
        let i = &step["inputs"];
        let now = f["inputs"]["now_s"].as_u64().expect("now_s");
        let built = register_request(
            &fixtures::constants(),
            now,
            fixtures::str(i, "push_reg_id"),
            fixtures::str(i, "uuid"),
        );
        fixtures::assert_request(&built, &step["request"]);
        fixtures::check_cases(step, |_, r| parse_device_id(r));
    }

    #[test]
    fn vehicles() {
        let f = fixtures::json("spa.json");
        let step = &f["vehicles"];
        with_session(&f, |s| {
            fixtures::assert_request(
                &vehicles_request(&fixtures::constants(), s),
                &step["request"],
            );
        });
        fixtures::check_cases(step, |_, r| parse_vehicles(r));
    }

    #[test]
    fn vehicles_never_carry_the_vin() {
        let f = fixtures::json("spa.json");
        let r = fixtures::response(&f["vehicles"]["responses"][0]["response"]);
        let json = serde_json::to_string(&parse_vehicles(&r).expect("vehicles")).expect("json");
        assert!(!json.contains("TESTVIN"), "{json}");
    }

    #[test]
    fn status() {
        let f = fixtures::json("spa.json");
        let step = &f["status"];
        with_session(&f, |s| {
            let c = fixtures::constants();
            fixtures::assert_request(
                &status_request(&c, s, &vehicle("veh-0001", 1)),
                &step["ccs2_request"],
            );
            fixtures::assert_request(
                &status_request(&c, s, &vehicle("veh-0002", 0)),
                &step["legacy_request"],
            );
        });
        fixtures::check_cases(step, |case, r| {
            parse_soc(r, case["ccs2"].as_u64().expect("ccs2") as u8)
        });
    }

    #[test]
    fn pin() {
        let f = fixtures::json("spa.json");
        let step = &f["pin"];
        with_session(&f, |s| {
            let built = pin_request(&fixtures::constants(), s, fixtures::str(step, "pin"));
            fixtures::assert_request(&built, &step["request"]);
        });
        fixtures::check_cases(step, |_, r| parse_control_token(r));
    }

    #[test]
    fn send_destination() {
        let f = fixtures::json("spa.json");
        let step = &f["send_destination"];
        let poi: Poi = serde_json::from_value(step["poi"].clone()).expect("poi");
        let control = Token {
            access_token: fixtures::str(&f["inputs"], "control_token").into(),
            expires_in_s: 600,
        };
        with_session(&f, |s| {
            let built = send_destination_request(
                &fixtures::constants(),
                s,
                &control,
                &vehicle("veh-0001", 1),
                &[poi],
            );
            fixtures::assert_request(&built, &step["request"]);
        });
        fixtures::check_cases(step, |_, r| parse_msg_id(r));
    }

    #[test]
    fn waypoints_are_numbered_in_order() {
        let f = fixtures::json("spa.json");
        let poi: Poi = serde_json::from_value(f["send_destination"]["poi"].clone()).expect("poi");
        let control = Token {
            access_token: "t".into(),
            expires_in_s: 1,
        };
        with_session(&f, |s| {
            let built = send_destination_request(
                &fixtures::constants(),
                s,
                &control,
                &vehicle("v", 1),
                &[poi.clone(), poi.clone()],
            );
            let body: Value =
                serde_json::from_str(built.body.as_deref().expect("body")).expect("json");
            let ids: Vec<u64> = body["poiInfoList"]
                .as_array()
                .expect("list")
                .iter()
                .map(|p| p["waypointID"].as_u64().expect("id"))
                .collect();
            assert_eq!(ids, vec![1, 2]);
        });
    }
}
