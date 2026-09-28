# Appointments

A line with @at is an appointment; xmd query --workspace 'events' lists appointments.

- Dentist @at(2026-09-25T09:30-04:00)
- Team lunch @at(2026-09-26T12:30-04:00)
- Flight home @at(2026-11-30T16:45-06:00)

next := 2026-09-25T09:30-04:00 - now()

The dentist is in [next].

<!-- Include an explicit UTC offset for times. Try: xmd query --workspace 'import("agenda").between(entries, today(), today() + 6d)'. -->
