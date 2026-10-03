use super::{
    DataConsumerHandle, DataConsumerIdentity, DataConsumerRequest, DataError, DataService,
};

/// Commit-scoped demand access. Producer payload and Host policy stay at the Host boundary.
/// Immutable service methods remain available through Deref.
pub struct DataConsumerAccess<'a> {
    pub(crate) service: &'a mut DataService,
}

impl std::ops::Deref for DataConsumerAccess<'_> {
    type Target = DataService;

    fn deref(&self) -> &Self::Target {
        self.service
    }
}

impl<'a> DataConsumerAccess<'a> {
    /// Apply a complete demand handoff before destructive expiry or source collection.
    /// The guard finishes on normal return, an error, or unwinding; scopes may nest.
    pub(crate) fn batch(self) -> DataConsumerBatch<'a> {
        let previously_deferred = self.service.begin_consumer_updates();
        DataConsumerBatch {
            access: self,
            previously_deferred,
        }
    }

    /// Register demand for a newly committed binding lifetime.
    pub fn register_consumer(
        &mut self,
        identity: DataConsumerIdentity,
        request: DataConsumerRequest,
    ) -> Result<DataConsumerHandle, DataError> {
        self.service.register_consumer(identity, request)
    }

    /// Change demand after authored admission succeeds.
    pub fn update_consumer(
        &mut self,
        handle: DataConsumerHandle,
        request: DataConsumerRequest,
    ) -> Result<(), DataError> {
        self.service.update_consumer(handle, request)
    }

    /// Release a departing binding's demand before its storage is reused.
    pub fn release_consumer(&mut self, handle: DataConsumerHandle) -> Result<(), DataError> {
        self.service.release_consumer(handle)
    }
}

/// Scoped consumer-only access; producer admission remains outside this boundary.
pub(crate) struct DataConsumerBatch<'a> {
    access: DataConsumerAccess<'a>,
    previously_deferred: bool,
}

impl<'a> std::ops::Deref for DataConsumerBatch<'a> {
    type Target = DataConsumerAccess<'a>;

    fn deref(&self) -> &Self::Target {
        &self.access
    }
}

impl std::ops::DerefMut for DataConsumerBatch<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.access
    }
}

impl Drop for DataConsumerBatch<'_> {
    fn drop(&mut self) {
        self.access
            .service
            .end_consumer_updates(self.previously_deferred);
    }
}
