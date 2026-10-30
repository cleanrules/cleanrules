import os

from hypothesis import HealthCheck, settings

# "dev" keeps the local loop fast; CI exports HYPOTHESIS_PROFILE=ci for a deeper search.
settings.register_profile("dev", max_examples=50, deadline=None)
settings.register_profile(
    "ci",
    max_examples=500,
    deadline=None,
    print_blob=True,
    suppress_health_check=[HealthCheck.too_slow],
)
settings.load_profile(os.environ.get("HYPOTHESIS_PROFILE", "dev"))
