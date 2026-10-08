"""Native vehicle tuning for converted cars, not GTA V handling.meta physics."""
import json
import math

# Keep aligned with sa_scene::vehicle::Handling::validate.
LIMITS = {
    'acceleration': (0.5, 20.0),
    'reverse_acceleration': (0.5, 10.0),
    'brake_deceleration': (1.0, 25.0),
    'top_speed': (5.0, 90.0),
    'reverse_speed': (1.0, 25.0),
    'tire_grip': (1.0, 12.0),
    'steering_lock': (0.1, 0.9),
    'steering_rate': (0.3, 4.0),
    'suspension_spring': (40.0, 250.0),
    'suspension_damping': (8.0, 40.0),
    'aerodynamic_drag': (0.0, 0.02),
}


def profile(path, read):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f'Duplicate native handling field: {key}')
            result[key] = value
        return result

    def invalid_constant(value):
        raise ValueError(f'Nonfinite native handling value: {value}')

    values = json.loads(read(path, 16 * 1024).decode('utf-8-sig'),
                        object_pairs_hook=unique, parse_constant=invalid_constant)
    if not isinstance(values, dict) or not values:
        raise ValueError('Native handling profile needs a nonempty JSON object')
    for name, value in values.items():
        if name not in LIMITS:
            raise ValueError(f'Unknown native handling field: {name}')
        low, high = LIMITS[name]
        if type(value) not in (int, float) or not low <= value <= high or not math.isfinite(value):
            raise ValueError(f'Native handling {name} must be a number between {low} and {high}')
    return values
