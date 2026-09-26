use serde::{Deserialize, Serialize};

use crate::UserId;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct User {
    pub id: UserId,
}
