FROM python:3.12-slim
WORKDIR /srv
COPY vendor/rwa-lp/service/requirements.txt .
COPY private/requirements.txt private-requirements.txt
RUN pip install --no-cache-dir -r requirements.txt -r private-requirements.txt
COPY vendor/rwa-lp/service/app ./app
COPY private ./private
CMD exec uvicorn private.main:app --host 0.0.0.0 --port ${PORT:-8080} --workers 1
