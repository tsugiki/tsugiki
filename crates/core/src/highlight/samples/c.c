#include "Python.h"
#define CHECK(op) (Py_IS_TYPE((op), &PyList_Type))

/* Returns the length, or -1 with an error set. */
static Py_ssize_t
list_length(PyObject *op)
{
    if (!CHECK(op)) {
        PyErr_SetString(PyExc_TypeError, "expected a list, not '%s'");
        return -1;
    }
    char sep = '\n';
    return PyList_GET_SIZE(op) + (sep == 0 ? 1 : 0);
}
