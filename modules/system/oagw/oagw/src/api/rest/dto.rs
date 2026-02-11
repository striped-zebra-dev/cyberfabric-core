// REST DTOs for the OAGW API.
//
// Currently re-exports SDK types since they serve as the REST contract.
// Dedicated REST-specific types can be added here as the API evolves.

pub use oagw_sdk::models::{
    CreateRouteRequest, CreateUpstreamRequest, Route, RouteResponse, UpdateRouteRequest,
    UpdateUpstreamRequest, Upstream, UpstreamResponse,
};
