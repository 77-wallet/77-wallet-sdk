pub(crate) struct GivenRole<'a, S: ?Sized> {
    scenario: &'a S,
}

impl<'a, S: ?Sized> GivenRole<'a, S> {
    pub(crate) fn new(scenario: &'a S) -> Self {
        Self { scenario }
    }

    pub(crate) fn scenario(&self) -> &'a S {
        self.scenario
    }
}

pub(crate) struct WhenRole<'a, S: ?Sized> {
    scenario: &'a S,
}

impl<'a, S: ?Sized> WhenRole<'a, S> {
    pub(crate) fn new(scenario: &'a S) -> Self {
        Self { scenario }
    }

    pub(crate) fn scenario(&self) -> &'a S {
        self.scenario
    }
}

pub(crate) struct ThenRole<'a, S: ?Sized> {
    scenario: &'a S,
}

impl<'a, S: ?Sized> ThenRole<'a, S> {
    pub(crate) fn new(scenario: &'a S) -> Self {
        Self { scenario }
    }

    pub(crate) fn scenario(&self) -> &'a S {
        self.scenario
    }
}

pub(crate) trait ScenarioRoles {
    fn given(&self) -> GivenRole<'_, Self>
    where
        Self: Sized,
    {
        GivenRole::new(self)
    }

    fn when(&self) -> WhenRole<'_, Self>
    where
        Self: Sized,
    {
        WhenRole::new(self)
    }

    fn then(&self) -> ThenRole<'_, Self>
    where
        Self: Sized,
    {
        ThenRole::new(self)
    }
}

impl<S> ScenarioRoles for S {}

pub(crate) struct SeedRole<'a, S: ?Sized> {
    scenario: &'a S,
}

impl<'a, S: ?Sized> SeedRole<'a, S> {
    pub(crate) fn new(scenario: &'a S) -> Self {
        Self { scenario }
    }

    pub(crate) fn scenario(&self) -> &'a S {
        self.scenario
    }
}

pub(crate) struct LoadRole<'a, S: ?Sized> {
    scenario: &'a S,
}

impl<'a, S: ?Sized> LoadRole<'a, S> {
    pub(crate) fn new(scenario: &'a S) -> Self {
        Self { scenario }
    }

    pub(crate) fn scenario(&self) -> &'a S {
        self.scenario
    }
}

#[allow(dead_code)]
pub(crate) struct CountRole<'a, S: ?Sized> {
    scenario: &'a S,
}

#[allow(dead_code)]
impl<'a, S: ?Sized> CountRole<'a, S> {
    pub(crate) fn new(scenario: &'a S) -> Self {
        Self { scenario }
    }

    pub(crate) fn scenario(&self) -> &'a S {
        self.scenario
    }
}

pub(crate) struct AssertRole<'a, S: ?Sized> {
    scenario: &'a S,
}

impl<'a, S: ?Sized> AssertRole<'a, S> {
    pub(crate) fn new(scenario: &'a S) -> Self {
        Self { scenario }
    }

    pub(crate) fn scenario(&self) -> &'a S {
        self.scenario
    }
}
